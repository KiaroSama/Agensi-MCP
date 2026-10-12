"""Small dependency-free release-policy regression tests."""
import importlib.util
import logging
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('release_check', Path(__file__).with_name('check-release.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class ReleaseTests(unittest.TestCase):
    def test_matching_release(self):
        module.validate('2.1.0', 'v2.1.0', 'refs/heads/main', True)
    def test_artifacts_on_feature_branch(self):
        module.validate('2.1.0', '', 'refs/heads/feature', True)
    def test_local_validation(self):
        module.validate('2.1.0', 'v2.1.0', '', False)
    def test_different_version(self):
        with self.assertRaises(ValueError): module.validate('2.1.0', 'v99.99.99', 'refs/heads/main', True)
    def test_unprefixed_tag(self):
        with self.assertRaises(ValueError): module.validate('2.1.0', '2.1.0', 'refs/heads/main', True)
    def test_shell_metacharacters(self):
        with self.assertRaises(ValueError): module.validate('2.1.0', 'v2.1.0;echo forbidden', 'refs/heads/main', True)
    def test_nonmain_release(self):
        with self.assertRaises(ValueError): module.validate('2.1.0', 'v2.1.0', 'refs/heads/feature', True)
    def test_prerelease_policy(self):
        with self.assertRaises(ValueError): module.validate('2.1.0-beta', 'v2.1.0-beta', 'refs/heads/main', True)

if __name__ == '__main__':
    logging.basicConfig(level=logging.INFO, format='%(asctime)s [%(levelname)s] %(message)s')
    logging.info('Running release identity regression tests.')
    unittest.main(verbosity=2)
