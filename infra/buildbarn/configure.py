#!/usr/bin/env python3
"""Offline, opt-in Buildbarn configuration renderer. Never contacts a build host."""
import argparse
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
import tempfile

UPSTREAM = '7cd419f6526ea5d1f25b0803dee32d64f26fbdec'
ALLOW = {'allow': {}}
MIB = 1024 ** 2
GIB = 1024 ** 3


def fail(message):
    raise ValueError(message)


def token(value, label):
    if not isinstance(value, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,95}', value):
        fail(f'{label}: expected a nonempty identifier')
    return value


def roster(text):
    result = {}
    for number, line in enumerate(text.splitlines(), 1):
        fields = line.split('#', 1)[0].split()
        if not fields:
            continue
        if not 3 <= len(fields) <= 5:
            fail(f'roster line {number}: expected name transport target [disk_GB [ram_GB]]')
        name, transport, target = fields[:3]
        token(name, 'host name')
        if name in result:
            fail(f'duplicate host: {name}')
        if transport not in {'ssh-unix', 'ssh-darwin', 'ssh-win32', 'instabox', 'salad'}:
            fail(f'{name}: unknown transport')
        for floor in fields[3:]:
            if not re.fullmatch(r'[0-9]+(?:\.[0-9]+)?', floor):
                fail(f'{name}: invalid resource floor')
        result[name] = {'transport': transport, 'target': target}
    return result


def path(value, label):
    if not isinstance(value, str) or not value.startswith('/') or '\n' in value or '\x00' in value or '..' in value.split('/'):
        fail(f'{label}: use an absolute Unix path without parent traversal')
    return value.rstrip('/') or '/'


def grpc(address):
    return {'grpc': {'client': {'address': address}, 'enableCompression': True}}


def server(address):
    return [{'listenAddresses': [address], 'authenticationPolicy': ALLOW}]


def local_store(root, size, chunks=False):
    config = {
        'keyLocationMap': {'onBlockDevice': {'file': {'path': root + '/key_location_map', 'sizeBytes': 400 * MIB if chunks else MIB}}, 'maximumGetAttempts': 16, 'maximumPutAttempts': 64},
        'oldBlocks': 8, 'currentBlocks': 24, 'newBlocks': 3 if chunks else 1,
        'blocksOnBlockDevice': {'source': {'file': {'path': root + '/blocks', 'sizeBytes': size}}, 'spareBlocks': 3},
        'persistent': {'stateDirectoryPath': root + '/persistent_state', 'minimumEpochInterval': '300s'},
    }
    if chunks:
        config['chunkingParameters'] = {'minChunkSizeBytes': 256 * 1024, 'horizonSizeBytes': 8 * 256 * 1024}
    return {'local': config}


def render(hosts, cfg):
    if set(cfg) != {'coordinator', 'workers'}:
        fail('profile requires exactly coordinator and workers')
    central = cfg['coordinator']
    if set(central) != {'address', 'state_dir'}:
        fail('coordinator requires address and state_dir')
    address = str(ipaddress.IPv4Address(central['address']))
    ip = ipaddress.ip_address(address)
    # No wildcard/public listeners. This is a trusted private-network bootstrap,
    # not an authenticated multi-tenant execution service.
    if not (ip.is_loopback or ip in ipaddress.ip_network('10.0.0.0/8') or ip in ipaddress.ip_network('172.16.0.0/12') or ip in ipaddress.ip_network('192.168.0.0/16') or ip in ipaddress.ip_network('100.64.0.0/10')):
        fail('coordinator address must be loopback, RFC1918 or CGNAT IPv4')
    state = path(central['state_dir'], 'coordinator state_dir')
    workers = cfg['workers']
    if not isinstance(workers, dict) or not workers:
        fail('at least one explicit worker is required')
    remote = grpc(f'{address}:8981')
    cas = {'chunkStorage': remote, 'chunkMappingStorage': {'chunkMappingValidating': {'backend': remote}}, 'cdcParameterCache': {'cacheSize': 1000, 'cacheDuration': '60s', 'cacheReplacementPolicy': 'LEAST_RECENTLY_USED'}}
    ac = {'completenessChecking': {'backend': remote, 'maximumTotalTreeSizeBytes': 64 * MIB}}
    base = {'maximumMessageSizeBytes': 2 * MIB}
    local_cas = {'chunkStorage': local_store(state + '/storage-cs', 32 * GIB, True), 'chunkMappingStorage': local_store(state + '/storage-cms', 20 * MIB)}
    cas_server = lambda backend: {'contentAddressableStorage': backend, 'getAuthorizer': ALLOW, 'putAuthorizer': ALLOW, 'findMissingAuthorizer': ALLOW, 'maximumChunkCount': 100000}
    files = {}
    files['coordinator/storage.json'] = dict(base, grpcServers=server(f'{address}:8981'), supportedCompressors=['ZSTD'], contentAddressableStorageServer=cas_server(local_cas), actionCache={'backend': local_store(state + '/storage-ac', 20 * MIB), 'getAuthorizer': ALLOW, 'putAuthorizer': ALLOW})
    files['coordinator/scheduler.json'] = dict(base, clientGrpcServers=server('127.0.0.1:8982'), workerGrpcServers=server(f'{address}:8983'), buildQueueStateGrpcServers=server('127.0.0.1:8984'), contentAddressableStorage=cas, executeAuthorizer=ALLOW, modifyDrainsAuthorizer=ALLOW, killOperationsAuthorizer=ALLOW, synchronizeAuthorizer=ALLOW, actionRouter={'simple': {'platformKeyExtractor': {'action': {}}, 'invocationKeyExtractors': [{'correlatedInvocationsId': {}}, {'toolInvocationId': {}}], 'initialSizeClassAnalyzer': {'defaultExecutionTimeout': '1800s', 'maximumExecutionTimeout': '7200s'}}}, platformQueueWithNoWorkersTimeout='60s')
    files['coordinator/frontend.json'] = dict(base, grpcServers=server(f'{address}:8980'), schedulers={'': {'endpoint': {'address': '127.0.0.1:8982'}}}, supportedCompressors=['ZSTD'], contentAddressableStorageServer=cas_server(cas), actionCache={'backend': ac, 'getAuthorizer': ALLOW, 'putAuthorizer': ALLOW}, executeAuthorizer=ALLOW)
    inventory_supplied = hosts is not None
    if hosts is None:
        hosts = {name: {} for name in workers}
    manifest = {'inventory_supplied': inventory_supplied, 'upstream': UPSTREAM, 'workers': {}, 'excluded': sorted(set(hosts) - set(workers)), 'coordinator_directories': [state + '/' + name + '/persistent_state' for name in ('storage-cs', 'storage-cms', 'storage-ac')]}
    for name, w in sorted(workers.items()):
        if name not in hosts:
            fail(f'{name}: absent from inventory')
        if set(w) != {'os', 'arch', 'toolchain', 'concurrency', 'state_dir'}:
            fail(f'{name}: requires os, arch, toolchain, concurrency, state_dir')
        if w['os'] not in ('linux', 'darwin') or w['arch'] not in ('x86_64', 'aarch64'):
            fail(f'{name}: only native Linux/macOS x86_64/aarch64 workers supported')
        expected = 'ssh-darwin' if w['os'] == 'darwin' else 'ssh-unix'
        token(name, 'worker name')
        if inventory_supplied and hosts[name]['transport'] != expected:
            fail(f'{name}: transport does not match explicit native OS')
        if type(w['concurrency']) is not int or not 1 <= w['concurrency'] <= 64:
            fail(f'{name}: concurrency must be an integer from 1 to 64')
        root = path(w['state_dir'], f'{name} state_dir')
        socket = root + '/runner.sock'
        if len(socket.encode()) > 100:
            fail(f'{name}: runner socket path exceeds portable limit')
        properties = {'OSFamily': w['os'], 'ISA': w['arch'], 'toolchain-id': token(w['toolchain'], 'toolchain')}
        runner = {'endpoint': {'address': 'unix:' + socket}, 'concurrency': w['concurrency'], 'maximumFilePoolFileCount': 10000, 'maximumFilePoolSizeBytes': GIB, 'platform': {'properties': [{'name': key, 'value': val} for key, val in sorted(properties.items())]}, 'workerId': {'hostname': name}}
        files[f'workers/{name}/runner.json'] = {'buildDirectoryPath': root + '/build', 'grpcServers': [{'listenPaths': [socket], 'authenticationPolicy': ALLOW}]}
        files[f'workers/{name}/worker.json'] = dict(base, blobstore={'actionCache': ac, 'contentAddressableStorage': cas}, scheduler={'address': f'{address}:8983'}, buildDirectories=[{'native': {'buildDirectoryPath': root + '/build', 'cacheDirectoryPath': root + '/cache', 'maximumCacheFileCount': 10000, 'maximumCacheSizeBytes': GIB, 'cacheReplacementPolicy': 'LEAST_RECENTLY_USED'}, 'runners': [runner]}], filePool={'blockDevice': {'file': {'path': root + '/filepool', 'sizeBytes': GIB}}}, inputDownloadConcurrency=10, outputUploadConcurrency=10, directoryCache={'maximumCount': 1000, 'maximumSizeBytes': MIB, 'cacheReplacementPolicy': 'LEAST_RECENTLY_USED'})
        manifest['workers'][name] = dict(hosts[name], directories=[root + '/build', root + '/cache'], properties=properties)
        # One cell per capability profile, not per selected build host. Equal
        # properties route to all matching workers through the scheduler.
        profile = f"{w['os']}-{w['arch']}-{w['toolchain']}"
        prefix = f'buck2/{profile}/'
        files[prefix + '.buckroot'] = ''
        files[prefix + '.buckconfig'] = f'[cells]\nroot = .\n\n[buck2_re_client]\naction_cache_address = grpc://{address}:8980\nengine_address = grpc://{address}:8980\ncas_address = grpc://{address}:8980\ntls = false\ninstance_name =\n\n[build]\nexecution_platforms = root//:platforms\n'
        files[prefix + 'BUCK'] = 'load(":defs.bzl", "platforms", "smoke")\nplatforms(name = "platforms")\nsmoke(name = "smoke", nonce = read_config("smoke", "nonce", "initial"))\n'
        files[prefix + 'defs.bzl'] = buck_rules(properties)
    files['manifest.json'] = manifest
    return files


def buck_rules(properties):
    return '''# Isolated transport check: not a Cargo/Tauri build or performance benchmark.
def _platforms(ctx):
    platform = ExecutionPlatformInfo(
        label = ctx.label.raw_target(),
        configuration = ConfigurationInfo(constraints = {}, values = {}),
        executor_config = CommandExecutorConfig(
            local_enabled = False,
            remote_enabled = True,
            remote_execution_properties = %s,
            remote_execution_use_case = "buildbarn",
            remote_output_paths = "output_paths",
        ),
    )
    return [DefaultInfo(), ExecutionPlatformRegistrationInfo(platforms = [platform])]

platforms = rule(impl = _platforms, attrs = {})

def _smoke(ctx):
    outputs = []
    for index in range(8):
        output = ctx.actions.declare_output("worker-{}.txt".format(index))
        ctx.actions.run(
            ["/bin/sh", "-c", 'hostname > "$1" && printf "buildbarn-smoke-%%s-%%s\\n" "$2" "$3" >> "$1"', "--", output.as_output(), str(index), ctx.attrs.nonce],
            category = "cluster_smoke",
            identifier = str(index),
        )
        outputs.append(output)
    return [DefaultInfo(default_outputs = outputs)]

smoke = rule(impl = _smoke, attrs = {"nonce": attrs.string()})
''' % json.dumps(properties, sort_keys=True)


def write_output(destination, files):
    if destination.exists():
        fail('output already exists; choose a new path (never overwrite private configuration)')
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix='.buildbarn-', dir=destination.parent))
    try:
        for name, content in files.items():
            target = temporary / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content if isinstance(content, str) else json.dumps(content, indent=2, sort_keys=True) + '\n')
            target.chmod(0o600)
        temporary.rename(destination)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    inventory = parser.add_mutually_exclusive_group()
    inventory.add_argument('--hosts', type=Path, help='optional name/transport/target inventory')
    inventory.add_argument('--ferryx-inventory', action='store_true', help='opt in to FERRYX_BUILD_HOSTS_FILE or ~/.config/ferryx/build-hosts')
    parser.add_argument('--profile', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        hosts_path = args.hosts
        if args.ferryx_inventory:
            hosts_path = Path(os.environ.get('FERRYX_BUILD_HOSTS_FILE', str(Path.home() / '.config/ferryx/build-hosts')))
        elif hosts_path is None and os.environ.get('BUILDBARN_HOSTS_FILE'):
            hosts_path = Path(os.environ['BUILDBARN_HOSTS_FILE'])
        hosts = roster(hosts_path.read_text()) if hosts_path is not None else None
        files = render(hosts, json.loads(args.profile.read_text()))
        write_output(args.output, files)
    except (ValueError, OSError, TypeError, KeyError) as error:
        parser.exit(2, f'configuration error: {error}\n')
    print(f'Rendered {len(files)} files to {args.output}; no hosts contacted or services started.')


if __name__ == '__main__':
    main()
