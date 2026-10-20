# ---
# relationships:
#   demonstrates: template-interviews
# ---
Output docs/assets/document-input.gif
Require bash
Require jq
Set Shell "bash"
Set FontSize 20
Set Width 1250
Set Height 500
Set Theme "Catppuccin Mocha"
Set Padding 20
Set TypingSpeed 20ms
Hide
Type 'export PATH="$PWD/target/debug:$PATH"' Enter
Type 'DOCUMENT_DEMO=$(mktemp -d)' Enter
Type 'cp -R docs/examples/document "$DOCUMENT_DEMO/template"' Enter
Type 'cd "$DOCUMENT_DEMO"' Enter
Type 'export HOME="$DOCUMENT_DEMO/home" XDG_CONFIG_HOME="$DOCUMENT_DEMO/config" XDG_DATA_HOME="$DOCUMENT_DEMO/data" XDG_CACHE_HOME="$DOCUMENT_DEMO/cache" XDG_STATE_HOME="$DOCUMENT_DEMO/state"' Enter
Type 'export PS1="demo> "; clear' Enter
Show
Type 'cat template/catalog.json' Enter
Sleep 1s
Type 'toha apply ./template ./output --input ./template/catalog.json | jq "{status, files}"' Enter
Sleep 2s
Type 'cat output/catalog.txt' Enter
Sleep 3s
