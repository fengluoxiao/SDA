#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import struct
import unittest
spec=importlib.util.spec_from_file_location('desktop_verify',Path(__file__).with_name('verify-desktop-release.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
class DesktopArchitectureTests(unittest.TestCase):
 def test_windows_architectures_are_distinct(self):
  for cpu,arch in [(0x8664,'x64'),(0xaa64,'arm64')]:
   data=bytearray(512);data[:2]=b'MZ';struct.pack_into('<I',data,0x3c,128);data[128:132]=bytes([80,69,0,0]);struct.pack_into('<H',data,132,cpu)
   self.assertEqual(module.architectures(data),{arch})
 def test_macos_architectures_are_distinct(self):
  for cpu,arch in [(0x1000007,'x64'),(0x100000c,'arm64')]:
   data=bytearray(32);data[:4]=bytes([0xcf,0xfa,0xed,0xfe]);struct.pack_into('<I',data,4,cpu)
   self.assertEqual(module.architectures(data),{arch})
 def test_repo_relative_entrypoint_is_valid(self):
  import json
  pkg=json.loads((Path(__file__).resolve().parents[1]/'apps/desktop/package.json').read_text())
  module.validate_package_metadata(pkg,pkg['version'])
  module.validate_package_metadata({'version':pkg['version'],'main':'main.cjs'},pkg['version'])
  with self.assertRaises(AssertionError):module.validate_package_metadata({'version':pkg['version'],'main':'other.cjs'},pkg['version'])
 def test_invalid_executable_is_rejected(self):
  with self.assertRaises(ValueError):module.architectures(b'not an executable')
if __name__=='__main__':unittest.main()
