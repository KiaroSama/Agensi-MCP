"""Validate release identity before building or publishing any assets (CI only)."""
from __future__ import annotations
import argparse
import logging
import os
from pathlib import Path
import re
import tomllib

LOG = logging.getLogger('release')

def validate(version: str, tag: str, ref: str, in_ci: bool) -> None:
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version):
        raise ValueError('Package version is not a supported stable release version.')
    if tag and tag != 'v' + version:
        raise ValueError('Release tag must exactly match the Cargo package version.')
    if tag and in_ci and ref != 'refs/heads/main':
        raise ValueError('Versioned release publication requires the main branch.')

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=Path(__file__).resolve().parents[1] / 'Cargo.toml')
    parser.add_argument('--tag', default=os.environ.get('RELEASE_TAG', ''))
    parser.add_argument('--debug', action='store_true')
    args = parser.parse_args()
    logging.basicConfig(level=logging.DEBUG if args.debug else logging.INFO,
                        format='%(asctime)s [%(levelname)s] %(message)s')
    try:
        version = tomllib.loads(args.manifest.read_text(encoding='utf-8'))['package']['version']
        validate(version, args.tag, os.environ.get('GITHUB_REF', ''), os.environ.get('GITHUB_ACTIONS') == 'true')
        if output := os.environ.get('GITHUB_OUTPUT'):
            with open(output, 'a', encoding='utf-8') as stream:
                stream.write(f'version={version}\n')
        LOG.info('Release identity validated; version=%s mode=%s', version, 'release' if args.tag else 'checks/artifacts')
        return 0
    except (OSError, KeyError, TypeError, ValueError) as error:
        LOG.error('Release identity validation failed (%s); no assets should be published.', type(error).__name__)
        return 1

if __name__ == '__main__':
    raise SystemExit(main())
