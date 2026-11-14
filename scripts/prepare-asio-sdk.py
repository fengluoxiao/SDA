"""Stage a validated ASIO SDK for Windows builds without PowerShell archive modules."""
from pathlib import Path, PurePosixPath
import hashlib
import os
import tempfile
import urllib.request
import zipfile

SDK_URL = 'https://www.steinberg.net/asiosdk'
REQUIRED = ('common/asio.h', 'common/asiosys.h', 'host/asiodrivers.h', 'host/pc/asiolist.cpp')


def is_sdk(path):
    return all((Path(path) / name).is_file() for name in REQUIRED)


def find_sdk(directory):
    directory = Path(directory)
    candidates = sorted({directory} | {p.parent.parent for p in directory.rglob('asio.h') if p.parent.name == 'common'})
    matches = [p for p in candidates if is_sdk(p)]
    if len(matches) != 1:
        raise RuntimeError(f'Expected one complete ASIO SDK, found {len(matches)} in {directory}')
    return matches[0].resolve()


def extract_sdk(archive, destination):
    destination = Path(destination)
    with zipfile.ZipFile(archive) as source:
        for entry in source.infolist():
            name = entry.filename.replace('\\', '/')
            path = PurePosixPath(name)
            if path.is_absolute() or '..' in path.parts or ':' in name or '\n' in name or '\r' in name:
                raise RuntimeError('Invalid SDK archive path')
            if (entry.external_attr >> 16) & 0o170000 == 0o120000:
                raise RuntimeError('SDK archive must not contain symbolic links')
        source.extractall(destination)
    return find_sdk(destination)


def record_environment(sdk, environment_file):
    value = str(Path(sdk).resolve())
    if '\n' in value or '\r' in value:
        raise RuntimeError('Invalid SDK directory')
    with Path(environment_file).open('a', encoding='utf-8', newline='\n') as stream:
        stream.write('CPAL_ASIO_DIR=' + value + '\n')


def main():
    configured = os.environ.get('CPAL_ASIO_DIR')
    if configured:
        sdk = Path(configured).resolve()
        if not is_sdk(sdk):
            raise RuntimeError('CPAL_ASIO_DIR does not contain a complete ASIO SDK')
    else:
        base = Path(os.environ.get('RUNNER_TEMP', tempfile.gettempdir()))
        stage = Path(tempfile.mkdtemp(prefix='sda-asio-sdk-', dir=base))
        archive = stage / 'asio-sdk.zip'
        request = urllib.request.Request(SDK_URL, headers={'User-Agent': 'SDA-ASIO-build'})
        with urllib.request.urlopen(request, timeout=45) as response, archive.open('wb') as output:
            while chunk := response.read(1024 * 1024):
                output.write(chunk)
        print('ASIO SDK archive SHA256:', hashlib.sha256(archive.read_bytes()).hexdigest())
        sdk = extract_sdk(archive, stage / 'unpacked')
    if environment_file := os.environ.get('GITHUB_ENV'):
        record_environment(sdk, environment_file)
    print('CPAL_ASIO_DIR=' + str(sdk))


if __name__ == '__main__':
    main()
