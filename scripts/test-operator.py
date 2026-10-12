"""Static published-guidance contracts, not an agent effectiveness benchmark."""
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SKILL = ROOT / 'skills/agensi-operator'


class OperatorContracts(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.main = (SKILL/'SKILL.md').read_text(encoding='utf-8')
        cls.flow = (SKILL/'references/tools-and-workflows.md').read_text(encoding='utf-8')
        cls.policy = (SKILL/'references/policies.md').read_text(encoding='utf-8')
        cls.readme = (ROOT/'README.md').read_text(encoding='utf-8')

    def test_latest_use_without_acquired_files(self):
        self.assertIn('Fetch `get_skill` again for every requested use', self.flow)
        self.assertIn('Never save acquired instructions', self.flow)
        self.assertNotIn('Install free or owned one-time files', self.flow)
        for case in ('Use an owned skill again', 'Buy an unowned $3', 'Buy an $8', 'Add credit, amount missing', 'Post a review, rating missing', 'Save acquired content or try old free allowance'):
            self.assertIn(case, self.flow)

    def test_exact_payment_and_topup_consent(self):
        for phrase in ('exact slug, current price and payment route', 'buy_credits', '5 to 1000', 'No amount or approval: stop'):
            self.assertIn(phrase, self.flow)
        self.assertIn('$3 one-time skill: credit only', self.flow)
        self.assertIn('$8 one-time skill: card or credit', self.flow)
        self.assertIn('Ask for specific approval before', self.flow)
        self.assertIn('No `confirm=true` before specific approval', self.flow)
        self.assertIn('credits cannot buy subscription skills', self.flow)

    def test_rating_is_the_users_decision(self):
        for phrase in ('review_skill', "user's own rating", '1 to 5', 'Never invent or suggest a rating'):
            self.assertIn(phrase, self.flow)

    def test_current_money_rights_and_no_trial_fallback(self):
        for phrase in ('1 credit = $1', '12 months', '30 days', '70%', '$1', 'Starter and Pro', 'One connected agent'):
            self.assertIn(phrase, self.policy)
        self.assertIn('Do not use the retired free-run allowance', self.flow)
        self.assertIn('subscription skills', self.policy)
        self.assertIn('not a zero-persistence guarantee', self.policy)

    def test_original_authoring_is_not_marketplace_installation(self):
        self.assertIn('original or independently authorized content', self.readme)
        self.assertIn('not for acquired Agensi instructions', self.readme)
        self.assertIn('Existing local files are preserved', self.flow)
        self.assertIn('Original Skill Package', (ROOT/'CONTEXT.md').read_text(encoding='utf-8'))

    def test_native_plugin_versions_agree(self):
        self.assertIn('version: "1.3.0"', self.main)
        for client in ('claude', 'codex', 'cursor'):
            manifest=json.loads((ROOT/f'.{client}-plugin/plugin.json').read_text(encoding='utf-8'))
            self.assertEqual(manifest['version'], '1.3.0')
            self.assertEqual(manifest['skills'], './skills/')
        market=json.loads((ROOT/'.claude-plugin/marketplace.json').read_text(encoding='utf-8'))
        self.assertEqual(market['plugins'][0]['version'], '1.3.0')
        native=json.loads((ROOT/'plugin.json').read_text(encoding='utf-8'))
        self.assertEqual(native['name'], 'agensi-operator')


if __name__ == '__main__':
    unittest.main()
