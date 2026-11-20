"""Synthetic extraction regressions; no official binary/network required."""
import json
from pathlib import Path
import struct
import tempfile
import unittest

from claude_contract_extract import extract, digest
from claude_contract_p1 import minimal_env, sandbox_policy


class ExtractTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        source = b'var first=1;var second=2;'
        name = b'/$bunfs/root/sdk.js'
        graph = name+source
        table = len(graph)
        graph += struct.pack('<12I4B',0,len(name),len(name),len(source),0,0,0,0,0,0,0,0,1,1,0,0)
        trailer = b'\n---- Bun! ----\n'
        graph += struct.pack('<Q6I',len(graph),table,52,0,0,0,0)+trailer
        self.data = bytearray(256)+struct.pack('<Q',len(graph))+graph
        self.data[:8] = bytes.fromhex('cffaedfe0c000001')
        struct.pack_into('<I',self.data,16,1)
        struct.pack_into('<II',self.data,32,0x19,152)
        self.data[40:45] = b'__BUN'
        self.data[104:109] = b'__bun'
        struct.pack_into('<QI',self.data,144,len(graph)+8,256)
        self.binary = self.root/'binary'
        self.profile = {'binary_sha256':digest(self.data),'module':{'id':0,'name':name.decode(),
            'offset':264+len(name),'length':len(source),'sha256':digest(source)},
            'ranges':[{'name':'first','start':0,'end':12,'sha256':digest(source[:12])}]}
        self.path = self.root/'profile.json'
        self.save()

    def save(self):
        self.binary.write_bytes(self.data)
        self.path.write_text(json.dumps(self.profile))

    def execute(self):
        return extract(self.binary,self.root/'out',self.path)

    def test_valid_graph_and_provenance(self):
        manifest = self.execute()
        self.assertEqual(manifest['module']['offset'],self.profile['module']['offset'])
        self.assertEqual((self.root/'out/sdk-source.js').read_bytes(),b'var first=1;')

    def test_unknown_binary_and_changed_bytes(self):
        self.data[-1] ^= 1
        self.save()
        with self.assertRaisesRegex(ValueError,'unknown binary fingerprint'):
            self.execute()

    def test_missing_required_module(self):
        self.profile['module']['id'] = 1
        self.save()
        with self.assertRaisesRegex(ValueError,'missing required SDK module'):
            self.execute()

    def test_changed_source_range(self):
        self.profile['ranges'][0]['sha256'] = '0'*64
        self.save()
        with self.assertRaisesRegex(ValueError,'source range fingerprint'):
            self.execute()

    def test_invalid_table_bounds(self):
        struct.pack_into('<I',self.data,len(self.data)-len(b'\n---- Bun! ----\n')-24,0xFFFFFFFF)
        self.profile['binary_sha256'] = digest(self.data)
        self.save()
        with self.assertRaisesRegex(ValueError,'module table'):
            self.execute()

    def test_environment_does_not_inherit_credentials(self):
        env = minimal_env(self.root)
        self.assertEqual(set(env),{'PATH','HOME','TMPDIR','XDG_CONFIG_HOME','RUST_LOG','LANG'})
        self.assertEqual(env['HOME'],str(self.root))

    def test_policy_protects_real_credentials_and_keychain(self):
        policy = sandbox_policy(Path('/Users/example'),self.root/'canary')
        for item in ['(deny network*)','localhost:*','/usr/bin/security','com.apple.securityd',
                     '.codex/auth.json','.claude','Library/Keychains',str(self.root/'canary')]:
            self.assertIn(item,policy)


if __name__ == '__main__':
    unittest.main()
