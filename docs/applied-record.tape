# ---
# relationships:
#   demonstrates: applied-record
# ---
Output docs/assets/applied-record.gif
Require bash
Require jq
Require git
Set Shell "bash"
Set FontSize 20
Set Width 1280
Set Height 760
Set Theme "Catppuccin Mocha"
Set Padding 24
Set TypingSpeed 20ms
Hide
Type 'export PATH="$PWD/target/debug:$PATH"; export PS1="example> "' Enter
Type 'RECORD_DEMO=$(mktemp -d); cd "$RECORD_DEMO"; mkdir project; git -C project init -q -b main' Enter
Type 'export TOHA_CONFIG="$RECORD_DEMO/config.yml" TOHA_USER_CONFIG="$RECORD_DEMO/user.yml" XDG_DATA_HOME="$RECORD_DEMO/data" XDG_STATE_HOME="$RECORD_DEMO/state"' Enter
Type "echo '"
Type '{"template":"toha-demo","answers":{"title":"Example note","topic":"Planning"}}'
Type "' > answers.json" Enter
Type 'clear' Enter
Show
Type 'toha apply toha-demo project --answers answers.json | jq .record' Enter
Sleep 1s
Type "jq '.applications[] | {template, target, answers, paths}' project/.toha.json" Enter
Sleep 4s
Type 'git -C project add .; git -C project -c user.name="Example User" -c user.email="user@example.invalid" commit -qm "Generate project"' Enter
Sleep 1s
Type 'toha apply toha-demo project --update --answers answers.json | jq .status' Enter
Sleep 4s
