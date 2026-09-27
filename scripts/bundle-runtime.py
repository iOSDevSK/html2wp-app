"""Publish the tested desktop runtime to Docker Hub and pin its digest.

The name is retained for older release notes. This script no longer bundles an
archive into the desktop app. Docker credentials stay with the publisher's CLI.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / 'runtime'
FUNCTIONAL = ('Dockerfile', 'selfcheck.py', 'cloudflare.py', 'tools.json', 'h2g-files.sha256.json')


def functional_manifest():
    paths = [RUNTIME / name for name in FUNCTIONAL]
    paths += sorted((RUNTIME / 'skills').rglob('*'))
    if any(path.is_symlink() for path in paths):
        raise RuntimeError('Runtime build context contains a symlink')
    return {path.relative_to(RUNTIME).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in paths if path.is_file()}


def image_functional_manifest(image):
    code = '''from pathlib import Path
import hashlib,json
root=Path('/opt/desktop')
names=['Dockerfile','selfcheck.py','cloudflare.py','tools.json','h2g-files.sha256.json']
paths=[root/name for name in names]+sorted((root/'skills').rglob('*'))
print(json.dumps({p.relative_to(root).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in paths if p.is_file()},sort_keys=True))'''
    return json.loads(run('docker', 'run', '--rm', '--network', 'none', image, 'python3', '-c', code))


def run(*args, **kwargs):
    return subprocess.check_output(args, cwd=ROOT, text=True, **kwargs)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--namespace', required=True, help='Docker Hub username or organisation')
    parser.add_argument('--reuse-image', help='An already built local runtime with the same contents')
    args = parser.parse_args()
    namespace = args.namespace.lower()
    if not re.fullmatch(r'[a-z0-9][a-z0-9_-]{1,38}', namespace):
        parser.error('Invalid Docker Hub namespace')
    versions = json.loads((ROOT / 'runtime/versions.json').read_text())
    app_version = json.loads((ROOT / 'package.json').read_text())['version']
    run('node', 'scripts/prepare-runtime.mjs')
    inputs = functional_manifest()
    recipe = hashlib.sha256((json.dumps(inputs, sort_keys=True) +
                            f"\0{versions['codexVersion']}\0{versions['h2gCommit']}").encode()).hexdigest()[:12]
    tag = f'{namespace}/html2wp-runtime:desktop-{app_version}-{recipe}'
    if args.reuse_image:
        source = args.reuse_image
        if not re.fullmatch(r'html2wp-runtime:desktop-\d+\.\d+\.\d+-[0-9a-f]{12}', source):
            raise RuntimeError('Only a previous local html2wp runtime release can be reused')
        if image_functional_manifest(source) != inputs:
            raise RuntimeError('The existing image differs from this runtime build context; rebuild it')
    else:
        source = f'html2wp-runtime:desktop-{app_version}-{recipe}'
        subprocess.run(['docker', 'build', '--label', 'dev.html2wp.desktop=true', '-t', source, 'runtime'], cwd=ROOT, check=True)
    identity = json.loads(run('docker', 'image', 'inspect', source))[0]
    image_id = identity['Id']
    architecture = {'arm64': 'aarch64', 'amd64': 'x86_64'}[identity['Architecture']]
    if identity['Os'] != 'linux' or identity['Config']['Labels'].get('dev.html2wp.desktop') != 'true':
        raise RuntimeError('Refusing to publish an image without the expected runtime platform and label')
    if any(re.search(r'(TOKEN|SECRET|PASSWORD|API_KEY)', value.split('=', 1)[0], re.I)
           for value in identity['Config'].get('Env', [])):
        raise RuntimeError('Refusing to publish an image with a credential-like environment variable')
    codex = run('docker', 'run', '--rm', '--network', 'none', image_id, 'codex', '--version').strip()
    if codex.split()[-1] != versions['codexVersion']:
        raise RuntimeError('Image Codex version differs from the pinned version')
    check = run('docker', 'run', '--rm', '--network', 'none', image_id, 'python3', '/opt/desktop/selfcheck.py')
    if 'RUNTIME_OK' not in check:
        raise RuntimeError('Runtime self-check failed')
    run('docker', 'tag', image_id, tag)
    pushed = run('docker', 'push', tag)
    match = re.search(r'\bdigest: (sha256:[0-9a-f]{64})\b', pushed)
    if not match:
        raise RuntimeError('Docker Hub did not return a manifest digest')
    digest = match.group(1)
    reference = f'docker.io/{namespace}/html2wp-runtime@{digest}'
    local_after = json.loads(run('docker', 'image', 'inspect', tag))[0]
    if local_after['Id'] != image_id or not any(ref.endswith(f'@{digest}') for ref in local_after.get('RepoDigests', [])):
        raise RuntimeError('Pushed registry digest is not attached to the verified local image')
    child_digest = None
    with tempfile.TemporaryDirectory() as directory:
        (Path(directory) / 'config.json').write_text('{}\n')
        anonymous = {'DOCKER_CONFIG': directory, 'PATH': os.environ['PATH']}
        def public_manifest(ref):
            return json.loads(subprocess.check_output(
                ['docker', 'manifest', 'inspect', ref], cwd=ROOT, env=anonymous, text=True))
        manifest = public_manifest(reference)
        if 'manifests' in manifest:
            matching = [entry for entry in manifest['manifests']
                        if entry.get('platform', {}).get('os') == 'linux'
                        and entry['platform'].get('architecture') == identity['Architecture']]
            if len(matching) != 1:
                raise RuntimeError('Public Docker Hub index must contain exactly one matching Linux runtime')
            child = matching[0].get('digest', '')
            if not re.fullmatch(r'sha256:[0-9a-f]{64}', child):
                raise RuntimeError('Public Docker Hub index has an invalid runtime manifest digest')
            child_digest = child
            manifest = public_manifest(f'docker.io/{namespace}/html2wp-runtime@{child}')
    config_id = manifest.get('config', {}).get('digest')
    if not isinstance(config_id, str) or not re.fullmatch(r'sha256:[0-9a-f]{64}', config_id):
        raise RuntimeError('Public Docker Hub manifest has no valid image configuration digest')
    # Classic Docker reports the config digest as .Id; containerd may report
    # its manifest/index digest. Docker push above used the verified local tag,
    # and the anonymous public manifest supplies the portable config identity.
    identities = {image_id, digest, config_id}
    if child_digest:
        identities.add(child_digest)
    old = ROOT / 'runtime/prebuilt.json'
    if old.is_file():
        archived = json.loads(old.read_text())
        if archived.get('imageId') in identities:
            identities.update(archived.get('imageIds', []))
    release = {'schemaVersion': 1, 'published': True, 'image': reference,
               'imageId': config_id, 'imageIds': sorted(identities), 'architecture': architecture}
    (ROOT / 'runtime/runtime-release.json').write_text(json.dumps(release, indent=2) + '\n')
    versions['image'] = tag
    (ROOT / 'runtime/versions.json').write_text(json.dumps(versions, indent=2) + '\n')
    print(f'Published {reference}\nLocal image ID {image_id}; architecture {architecture}')


if __name__ == '__main__':
    main()
