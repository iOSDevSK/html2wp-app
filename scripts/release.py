"""Compatibility entry point for staging a desktop release.

The former script created source bundles. Public updater releases must contain
only signed binary assets, so publication is performed separately after review.
"""
import runpy
from pathlib import Path

runpy.run_path(str(Path(__file__).with_name('prepare-desktop-release.py')), run_name='__main__')
