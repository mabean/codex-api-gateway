"""Pinned Mach-O/Bun extraction. Never invokes the supplied executable."""
import hashlib
import json
from pathlib import Path
import struct

PROFILE = Path(__file__).parent / 'fixtures/claude_contract/2.1.329-macos-arm64.json'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def extract(binary, output, profile_path=PROFILE):
    profile = json.loads(Path(profile_path).read_text())
    data = Path(binary).read_bytes()
    if digest(data) != profile['binary_sha256']:
        raise ValueError('unknown binary fingerprint: expected pinned Claude Code 2.1.329 darwin-arm64')
    if data[:8] != bytes.fromhex('cffaedfe0c000001'):
        raise ValueError('expected arm64 Mach-O')
    command = 32
    section = None
    for _ in range(struct.unpack_from('<I', data, 16)[0]):
        kind, span = struct.unpack_from('<II', data, command)
        if span < 8 or command + span > len(data):
            raise ValueError('invalid Mach-O command bounds')
        if kind == 0x19 and data[command+8:command+24].rstrip(b'\0') == b'__BUN':
            if data[command+72:command+88].rstrip(b'\0') != b'__bun':
                raise ValueError('missing Bun section')
            section_size, section = struct.unpack_from('<QI', data, command+112)
            break
        command += span
    if section is None:
        raise ValueError('missing Bun graph')
    start = section + 8
    end = start + struct.unpack_from('<Q', data, section)[0]
    trailer = b'\n---- Bun! ----\n'
    if not start < end <= section + section_size <= len(data) or data[end-len(trailer):end] != trailer:
        raise ValueError('invalid Bun graph bounds/trailer')
    size, table, table_size, *_ = struct.unpack_from('<Q6I', data, end-len(trailer)-32)
    if start + size + 32 + len(trailer) != end or table_size % 52 or table+table_size > size:
        raise ValueError('invalid Bun module table')
    module = profile['module']
    if module['id'] >= table_size // 52:
        raise ValueError('missing required SDK module')
    row = struct.unpack_from('<12I4B', data, start+table+module['id']*52)
    no, nl, so, sl = row[:4]
    if max(no+nl, so+sl) > size or row[-4:-2] != (1, 1):
        raise ValueError('invalid required SDK module source')
    if (data[start+no:start+no+nl].decode() != module['name'] or
            start+so != module['offset'] or sl != module['length']):
        raise ValueError('missing or relocated required SDK module')
    source = data[start+so:start+so+sl]
    if digest(source) != module['sha256']:
        raise ValueError('SDK source fingerprint mismatch')
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    chunks = []
    for part in profile['ranges']:
        a, b = part['start'], part['end']
        if not 0 <= a < b <= len(source) or digest(source[a:b]) != part['sha256']:
            raise ValueError('required source range fingerprint mismatch: '+part['name'])
        chunks.append(source[a:b])
    (output/'sdk-source.js').write_bytes(b'\n'.join(chunks))
    manifest = dict(profile, binary_path=str(Path(binary).resolve()),
                    executed_layer='embedded beta Messages SDK; not main CLI',
                    source_file='sdk-source.js')
    (output/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    return manifest
