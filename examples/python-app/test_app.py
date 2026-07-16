import unittest

from app import greeting


class GreetingTest(unittest.TestCase):
    def test_mentions_flux(self):
        self.assertIn("Flux", greeting())


if __name__ == "__main__":
    unittest.main()
