"""Synthetic controls for the recorded-demo audit; never opens a vault."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('demo_audit', Path(__file__).parents[1]/'audit-mcp-demo.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class DemoAudit(unittest.TestCase):
    def fixture(self, directory, *, changed=False, filled=True, canary=False):
        records = [{'credential_ref':'synthetic-reference','label':'Example','field_names':['password']}]
        calls = [('list_credentials','credentials',records),
                 ('secure_prompt_fill','fill',{'state':'pending'}),
                 ('fill_status','fill',{'state':'filled' if filled else 'pending'}),
                 ('list_credentials','credentials',[] if changed else records)]
        rows = []
        for method, kind, data in calls:
            rows.append({'timestamp':'synthetic','type':'response_item','payload':{'type':'custom_tool_call',
                'input':f'text(await tools.mcp__magicvault_jit_demo__{method}({{}}));'}})
            result = {'kind':kind,'data':data}
            if canary and method == 'fill_status':
                result['synthetic_control'] = audit.CANARIES['public password']
            output = {'content':[{'type':'text','text':json.dumps(result)}]}
            rows.append({'timestamp':'synthetic','type':'response_item','payload':{'type':'custom_tool_call_output',
                'output':[{'type':'text','text':json.dumps(output)}]}})
        path = Path(directory)/'synthetic.jsonl'
        path.write_text('\n'.join(json.dumps(row) for row in rows)+'\n')
        return path

    def test_jit_requires_real_completion_and_unchanged_metadata(self):
        with tempfile.TemporaryDirectory() as root:
            report = audit.audit(self.fixture(root), 'magicvault_jit_demo', True)
            self.assertTrue(report['jit_evidence']['saved_metadata_unchanged'])
            self.assertFalse(any(n for c in report['counts'].values() for n in c.values()))
            for kwargs in ({'changed':True}, {'filled':False}):
                with self.assertRaises(ValueError):
                    audit.audit(self.fixture(root, **kwargs), 'magicvault_jit_demo', True)

    def test_positive_control_and_wrong_namespace(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.fixture(root, canary=True)
            report = audit.audit(path, 'magicvault_jit_demo', True)
            self.assertGreater(report['counts']['public password']['mcp_replies'], 0)
            self.assertGreater(report['counts']['public password']['entire_rollout'], 0)
            with self.assertRaises(ValueError):
                audit.audit(path, 'different_server', True)


if __name__ == '__main__':
    unittest.main()
