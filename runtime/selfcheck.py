"""The runtime image holds what the agent's shell and the skills need."""
from pathlib import Path
import shutil
from importlib.util import find_spec
for program in ('node','npm','python3','php','wp','bash','jq','zip','unzip','rsync','curl','git','codex','docker'):
    assert shutil.which(program),f'Missing {program}'
for module in ('playwright','PIL','numpy'):assert find_spec(module),f'Missing {module}'
import subprocess
assert subprocess.run(['docker','compose','version'],capture_output=True).returncode==0,'Missing the docker compose plugin'
# The html2wp plugin is not in the image: the app fetches it from GitHub and
# mounts it read-only at /opt/html2wp.
# Gutenberg from an HTML theme: its skill and the offline WordPress.
assert Path('/opt/desktop/skills/html2wp-to-gutenberg/SKILL.md').is_file(),'Missing html2wp-to-gutenberg'
assert Path('/opt/wp-offline/wordpress/wp-includes/version.php').is_file(),'Missing the offline WordPress'
print('RUNTIME_OK')
