import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('runtime_archive', Path(__file__).resolve().parents[1] / 'scripts/runtime_archive.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class RuntimeArchiveTests(unittest.TestCase):
    def fixture(self, oci=True, corrupt=False):
        config = json.dumps({'architecture': 'arm64', 'os': 'linux', 'rootfs': {'type': 'layers', 'diff_ids': []}}).encode()
        config_hash = hashlib.sha256(config).hexdigest()
        config_path = 'blobs/sha256/' + config_hash if oci else config_hash + '.json'
        manifest = json.dumps({'schemaVersion': 2, 'config': {'digest': 'sha256:' + config_hash}, 'layers': []}).encode()
        manifest_hash = hashlib.sha256(manifest).hexdigest()
        files = {config_path: config, 'manifest.json': json.dumps([{'Config': config_path, 'RepoTags': ['runtime:test'], 'Layers': []}]).encode()}
        ids = {'sha256:' + config_hash}
        if oci:
            files['blobs/sha256/' + manifest_hash] = manifest + (b' ' if corrupt else b'')
            ids.add('sha256:' + manifest_hash)
            index = json.dumps({'schemaVersion': 2, 'manifests': [{'digest': 'sha256:' + manifest_hash}]}).encode()
            index_hash = hashlib.sha256(index).hexdigest()
            files['blobs/sha256/' + index_hash] = index
            files['index.json'] = json.dumps({'schemaVersion': 2, 'manifests': [{'digest': 'sha256:' + index_hash}]}).encode()
            ids.add('sha256:' + index_hash)
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode='w:gz') as archive:
            for name, data in files.items():
                entry = tarfile.TarInfo(name)
                entry.size = len(data)
                archive.addfile(entry, io.BytesIO(data))
        return output.getvalue(), ids

    def check_archive(self, **options):
        payload, identities = self.fixture(**options)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'runtime.tar.gz'
            path.write_bytes(payload)
            result = module.archive_identities(path, 'runtime:test', 'aarch64')
            self.assertEqual(set(result['imageIds']), identities)
            self.assertIn(result['configId'], identities)
            with self.assertRaisesRegex(ValueError, 'platform'):
                module.archive_identities(path, 'runtime:test', 'x86_64')
            with self.assertRaisesRegex(ValueError, 'expected image tag'):
                module.archive_identities(path, 'unrelated:test', 'aarch64')

    def test_classic_config_identity(self):
        self.check_archive(oci=False)

    def test_containerd_manifest_and_index_identities(self):
        self.check_archive()

    def test_corrupted_descriptor_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'checksum'):
            self.check_archive(corrupt=True)
