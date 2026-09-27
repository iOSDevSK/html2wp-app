"""Derive Docker image identities from a saved single-platform runtime archive.

Classic Docker uses the config digest; containerd uses the manifest/index digest.
Only identities backed by the exported bytes belong in the desktop's allowlist.
"""
import hashlib
import json
import tarfile


def archive_identities(path, image, architecture):
    documents = {}
    with tarfile.open(path, 'r|*') as archive:
        for member in archive:
            metadata = member.name in ('manifest.json', 'index.json') or member.name.startswith('blobs/sha256/') or member.name.endswith('.json')
            if not member.isfile() or not metadata or member.size > 4 * 1024 * 1024:
                continue
            raw = archive.extractfile(member).read()
            try:
                value = json.loads(raw)
            except (ValueError, UnicodeDecodeError):
                continue  # Small layer tarballs are not image metadata.
            if member.name in documents:
                raise ValueError('Duplicate runtime metadata entry')
            documents[member.name] = (raw, value)

    def blob(name):
        raw, value = documents[name]
        digest = hashlib.sha256(raw).hexdigest()
        if name.removesuffix('.json').split('/')[-1] != digest:
            raise ValueError('Runtime metadata blob checksum did not match')
        return 'sha256:' + digest, value

    entries = [entry for entry in documents['manifest.json'][1]
               if image in entry.get('RepoTags', [])]
    if len(entries) != 1:
        raise ValueError('Export exactly one runtime platform with the expected image tag')
    config_id, config = blob(entries[0]['Config'])
    expected_arch = {'aarch64': 'arm64', 'x86_64': 'amd64'}[architecture]
    if config.get('architecture') != expected_arch or config.get('os') != 'linux':
        raise ValueError('Runtime archive platform did not match')
    identities = {config_id}
    indexes = []
    for name, (_, value) in documents.items():
        if not name.startswith('blobs/sha256/') or not isinstance(value, dict):
            continue
        if value.get('config', {}).get('digest') == config_id:
            digest, _ = blob(name)
            identities.add(digest)
        elif value.get('manifests'):
            indexes.append(name)
    # Include an exported index only when every child is this verified runtime.
    for _ in range(len(indexes)):
        for name in indexes:
            if all(child.get('digest') in identities for child in documents[name][1]['manifests']):
                digest, _ = blob(name)
                identities.add(digest)
    return {'configId': config_id, 'imageIds': sorted(identities)}
