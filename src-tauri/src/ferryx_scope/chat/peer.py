import json, sys
def send(v):
    print(json.dumps(v), flush=True)
for line in sys.stdin:
    m = json.loads(line)
    method = m.get('method')
    if method == 'initialize':
        send({'id':m['id'],'result':{'userAgent':'codex/0.153.2'}})
    elif method == 'thread/start':
        send({'id':m['id'],'result':{'thread':{'id':'thread-qa'}}})
    elif method == 'turn/start':
        send({'id':m['id'],'result':{'turn':{'id':'turn-qa'}}})
        send({'id':'callback-real','method':'item/commandExecution/requestApproval','params':{'threadId':'thread-qa','turnId':'turn-qa','itemId':'cmd','command':'true'}})
    elif m.get('id') == 'callback-real':
        assert m['result']['decision'] in ['accept','decline']
        send({'id':17,'method':'item/tool/requestUserInput','params':{'threadId':'thread-qa','turnId':'turn-qa','itemId':'q','isBlocking':True,'questions':[{'id':'choice','header':'Choose','question':'Which?','options':[{'label':'A','description':'a'}]}]}})
    elif m.get('id') == 17:
        send({'method':'fixture/answered','params':m['result']})
        sys.exit(0)
