"""Regression checks for Hugging Face download progress compatibility.

Run from the repository root: python -m unittest tools.test_model_progress
"""

import io
import unittest

from huggingface_hub.utils import disable_progress_bars, enable_progress_bars

from src.model_host import _progress_tqdm


class ModelProgressTests(unittest.TestCase):
    def test_named_download_reports_progress(self):
        reports = []
        with _progress_tqdm(reports.append)(
            total=100, name="huggingface_hub.snapshot_download", file=io.StringIO()
        ) as progress:
            progress.update(25)
        self.assertEqual(reports, ["downloading model 25%"])

    def test_named_progress_can_be_disabled(self):
        name = "eqho.regression.download"
        disable_progress_bars(name)
        try:
            with _progress_tqdm(lambda _: None)(
                total=100, name=name, file=io.StringIO()
            ) as progress:
                self.assertTrue(progress.disable)
        finally:
            enable_progress_bars(name)


if __name__ == "__main__":
    unittest.main()
