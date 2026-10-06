#!/usr/bin/env bash
# Throwaway spike: reproduce the Jinja compatibility probe over the sample corpus.
# Expects shallow clones in /tmp/eco/<owner>_<repo> (see README.md).
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
eco=/tmp/eco; out=/tmp/eco-out; probe=/tmp/eco-probe
rm -rf "$out" "$probe"; mkdir -p "$out" "$probe/plain" "$probe/py"
copier_data='{"project_name":"Widget Tool","package_name":"widget_tool","project_description":"A widget tool.","author_fullname":"Example Author","author_name":"Example Author","author_username":"example","author_email":"author@example.com","repository_namespace":"example","copyright_holder":"Example","copyright_holder_email":"author@example.com","full_name":"Example Author","email":"author@example.com","keywords":"widget","package_short_description":"A widget tool.","github_organization":"example","code_repository":"https://github.com/example/widget","project_url":"https://github.com/example/widget","package_description":"A widget tool."}'
feed() { # kind template [extra-json]
  local t=$2; shift 2
  OUT_JSON="$probe/$t.json" PYTHONPATH="$eco/$t" timeout 300 uv run -q \
    --with cookiecutter --with copier --with copier-templates-extensions \
    --with jinja2-time --with jinja2-ansible-filters \
    python "$here/feed.py" "$kind" "$eco/$t" "$out/$t" "$@" 2>"$probe/$t.err" </dev/null >/dev/null
  tail -1 "$probe/$t.err"
}
kind=cookiecutter
feed $kind audreyfeldroy_cookiecutter-pypackage
feed $kind cookiecutter_cookiecutter-django
feed $kind ionelmc_cookiecutter-pylibrary
feed $kind simonw_click-app '{"app_name":"widget tool","description":"d","github_username":"example","author_name":"Example"}'
CONTEXT_FILE=ccds.json feed $kind drivendata_cookiecutter-data-science
kind=copier
for t in pawamoy_copier-uv superlinear-ai_substrate NLeSC_python-template Tecnativa_doodba-copier-template; do
  feed $kind $t "$copier_data"
done
for f in "$probe"/*.json; do
  n=$(basename "$f" .json)
  "$here/jinja-probe/target/debug/jinja-probe" <"$f" >"$probe/plain/$n.jsonl"
  "$here/jinja-probe/target-py/debug/jinja-probe" <"$f" >"$probe/py/$n.jsonl"
done
