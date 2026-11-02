"""Frozen portfolio script requests: no real service or credentials."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[1]
SCRIPT=ROOT/'skills/wealthlog/scripts/wealthlog_portfolio.py'

class PortfolioScriptTests(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory(prefix='.portfolio-script-test-',dir=ROOT)
        self.root=Path(self.directory.name)
        self.token=self.root/'token';self.token.write_text('test-secret-not-for-output\n');self.token.chmod(0o600)
        self.log=self.root/'calls.jsonl';self.fake=self.root/'wealthlog'
        self.fake.write_text('''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
args=sys.argv[1:]
p=Path(os.environ['TEST_CALL_LOG'])
with p.open('a') as f:f.write(json.dumps(args)+'\\n')
if 'status' in args: print(json.dumps({'ok':True,'data':{'state':'ready'}}))
elif '--input' in args:
 body=json.loads(Path(args[args.index('--input')+1]).read_text())
 if os.environ.get('TEST_UNKNOWN'):
  print(json.dumps({'ok':False,'error':{'code':'result_unknown'}}));sys.exit(5)
 print(json.dumps({'ok':True,'requestKey':body['requestKey'],'data':{'result':{'requestId':'req-fixed','replayed':False}}}))
else:print(json.dumps({'ok':True,'data':{'stateVersion':'version-1','cashBalances':{'CNY':'63007.83'}}}))
''');self.fake.chmod(0o700)
        self.config=self.root/'config.json';self.state=self.root/'requests'
        self.config.write_text(json.dumps({'cli':str(self.fake),'appId':'local.wealthlog.test','ledgerId':'11111111-1111-4111-8111-111111111111','tokenFile':str(self.token),'stateDir':str(self.state),'timezone':'Asia/Shanghai'}))
        self.payload=self.root/'input.json';self.payload.write_text(json.dumps({'accountId':'synthetic','activityDate':'2026-10-29','quantity':'3000','price':'1.298','expectedStateVersion':'version-1'}))
        self.env={**os.environ,'TEST_CALL_LOG':str(self.log),'WEALTHLOG_AGENT_STATE_DIR':str(self.state),'PYTHONDONTWRITEBYTECODE':'1'}
    def tearDown(self):self.directory.cleanup()
    def run_script(self,*args,extra=None):
        p=subprocess.run([sys.executable,str(SCRIPT),'--config',str(self.config),*args],cwd=ROOT,env={**self.env,**(extra or {})},capture_output=True,text=True)
        self.assertNotIn('test-secret-not-for-output',p.stdout+p.stderr)
        return p.returncode,json.loads(p.stdout)
    def frozen(self):
        code,value=self.run_script('prepare','manual_trade_record','--input',str(self.payload))
        self.assertEqual(code,0);return value
    def test_reads_use_bound_cli_without_tokens_on_command_line(self):
        code,value=self.run_script('state','--account-id','synthetic')
        self.assertEqual(code,0);self.assertEqual(value['data']['cashBalances']['CNY'],'63007.83')
        calls=[json.loads(v) for v in self.log.read_text().splitlines()]
        self.assertEqual(len(calls),2)
        self.assertIn('--ledger',calls[1]);self.assertIn('--token-file',calls[1])
        self.assertNotIn('test-secret-not-for-output',self.log.read_text())
    def test_freeze_does_not_submit_and_retries_keep_the_same_body(self):
        frozen=self.frozen();self.assertFalse(self.log.exists())
        path=Path(frozen['requestPath']);before=path.read_bytes()
        self.assertEqual(path.stat().st_mode&0o777,0o600)
        for _ in range(2):
            code,value=self.run_script('submit','--request-dir',frozen['requestDir'])
            self.assertEqual(code,0);self.assertEqual(value['requestKey'],frozen['requestKey'])
        self.assertEqual(before,path.read_bytes())
        status=(path.parent/'status').read_text().strip()
        self.assertEqual(status,'resolved')
    def test_changed_payload_or_credential_never_reaches_service(self):
        frozen=self.frozen();path=Path(frozen['requestPath']);payload=json.loads(path.read_text());payload['quantity']='9000';path.write_text(json.dumps(payload))
        code,_=self.run_script('submit','--request-dir',frozen['requestDir'])
        self.assertNotEqual(code,0);self.assertFalse(self.log.exists())
    def test_changed_credential_never_reaches_service(self):
        frozen=self.frozen();self.token.write_text('different-test-secret\n')
        code,_=self.run_script('submit','--request-dir',frozen['requestDir'])
        self.assertNotEqual(code,0);self.assertFalse(self.log.exists())

    def test_unknown_outcome_is_retained_without_automatic_retry(self):
        frozen=self.frozen()
        code,value=self.run_script('submit','--request-dir',frozen['requestDir'],extra={'TEST_UNKNOWN':'1'})
        self.assertEqual(code,5);self.assertEqual(value['error']['code'],'result_unknown')
        calls=self.log.read_text().splitlines();self.assertEqual(len(calls),2)
        self.assertEqual((Path(frozen['requestDir'])/'status').read_text().strip(),'unknown')

if __name__=='__main__':unittest.main()
