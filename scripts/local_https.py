#!/usr/bin/env python3
"""Generate fresh local-only HTTPS credentials and copy the runnable examples."""
import argparse
from pathlib import Path
import shutil
from test_hosted_http import credentials, ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path, help='New directory for the local CA, identity, and examples')
    args = parser.parse_args()
    directory = args.directory.resolve()
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    credentials(directory)
    for name in ('https_client', 'https_server'):
        shutil.copyfile(ROOT / f'examples/{name}.dodo', directory / f'{name}.dodo')
    print(f'Created {directory}. Certificates are for localhost and expire in one day.')
    print('From that directory: dodo build https_server.dodo -o server; dodo build https_client.dodo -o client')
    print('Start ./server, then run ./client in another terminal. Stop the server with Ctrl-C.')


if __name__ == '__main__':
    main()
