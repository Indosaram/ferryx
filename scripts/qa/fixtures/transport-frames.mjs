/** Incremental logical-frame decoding. TCP chunk hashes are never delivery IDs. */
import { createHash } from 'node:crypto';
import { TransportBarrier } from './transport-provider.mjs';

export class TransportFrames {
  constructor(protocol) {
    if (!['helper', 'websocket'].includes(protocol)) {
      throw new TransportBarrier('WIRE_PROTOCOL_REQUIRED', protocol);
    }
    this.protocol = protocol;
    this.buffer = Buffer.alloc(0);
    this.fragments = [];
    this.fragmentOpcode = null;
  }

  push(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    const frames = [];
    const receipt = (payload, opcode) => {
      let message;
      if (opcode === 1 || this.protocol === 'helper') message = JSON.parse(payload.toString('utf8'));
      frames.push({ message, opcode, payloadSha256: createHash('sha256').update(payload).digest('hex'),
        payloadLength: payload.length });
    };
    while (true) {
      if (this.protocol === 'helper') {
        if (this.buffer.length < 4) break;
        const length = this.buffer.readUInt32BE();
        if (length > 1024 * 1024) throw new TransportBarrier('FRAME_TOO_LARGE', 'helper <= 1 MiB');
        if (this.buffer.length < length + 4) break;
        receipt(this.buffer.subarray(4, length + 4), 1);
        this.buffer = this.buffer.subarray(length + 4);
        continue;
      }
      if (this.buffer.length < 2) break;
      const first = this.buffer[0];
      const second = this.buffer[1];
      if (first & 0x70) throw new TransportBarrier('WS_EXTENSION_UNSUPPORTED', 'uncompressed fixture WS');
      const fin = Boolean(first & 0x80);
      const opcode = first & 0x0f;
      let length = second & 0x7f;
      let header = 2;
      if (length === 126) {
        if (this.buffer.length < 4) break;
        length = this.buffer.readUInt16BE(2); header = 4;
      } else if (length === 127) {
        if (this.buffer.length < 10) break;
        const wide = this.buffer.readBigUInt64BE(2);
        if (wide > 1048576n) throw new TransportBarrier('FRAME_TOO_LARGE', 'fixture WS <= 1 MiB');
        length = Number(wide); header = 10;
      }
      if (length > 1024 * 1024) throw new TransportBarrier('FRAME_TOO_LARGE', 'fixture WS <= 1 MiB');
      const masked = Boolean(second & 0x80);
      const offset = header + (masked ? 4 : 0);
      if (this.buffer.length < offset + length) break;
      const payload = Buffer.from(this.buffer.subarray(offset, offset + length));
      if (masked) for (let i = 0; i < length; i++) payload[i] ^= this.buffer[header + i % 4];
      this.buffer = this.buffer.subarray(offset + length);
      if (opcode >= 8) {
        if (!fin || length > 125) throw new TransportBarrier('INVALID_WS_CONTROL', String(opcode));
        continue;
      }
      if (![0, 1, 2].includes(opcode)) throw new TransportBarrier('INVALID_WS_OPCODE', String(opcode));
      if (opcode !== 0) {
        if (this.fragmentOpcode !== null) throw new TransportBarrier('INVALID_WS_FRAGMENT', 'interleaved data messages');
        this.fragmentOpcode = opcode;
      } else if (this.fragmentOpcode === null) throw new TransportBarrier('INVALID_WS_FRAGMENT', 'continuation without start');
      this.fragments.push(payload);
      if (this.fragments.reduce((sum, b) => sum + b.length, 0) > 1024 * 1024) {
        throw new TransportBarrier('MESSAGE_TOO_LARGE', 'fixture WS <= 1 MiB');
      }
      if (fin) {
        receipt(Buffer.concat(this.fragments), this.fragmentOpcode);
        this.fragments = []; this.fragmentOpcode = null;
      }
    }
    return frames;
  }
}
