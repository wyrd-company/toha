# ---
# relationships:
#   demonstrates: getting-started
#   references: agents
# ---
Output docs/assets/demo.gif

Require bash
Require jq
Require tree

Set Shell "bash"
Set FontSize 22
Set Width 1280
Set Height 800
Set Theme "Catppuccin Mocha"
Set Padding 24
Set Margin 12
Set MarginFill "#11111b"
Set BorderRadius 8
Set WindowBar "Colorful"
Set TypingSpeed 24ms

Hide
Type 'unset NO_COLOR; export TERM=xterm-256color COLORTERM=truecolor LS_COLORS="di=01;34:ex=01;32"' Enter
Type 'export PATH="$PWD/target/debug:$PATH"' Enter
Type 'export PS1="demo> "' Enter
Type 'DEMO_ROOT=$(mktemp -d); cp -R docs/examples/demo "$DEMO_ROOT/template"; cd "$DEMO_ROOT"' Enter
Type 'export XDG_STATE_HOME="$DEMO_ROOT/state"' Enter
Type 'clear' Enter
Show

Sleep 500ms
Type 'toha apply ./template ./notes' Enter
Sleep 600ms
Type 'Sample note' Enter
Sleep 300ms
Type 'Planning' Enter
Sleep 1500ms
Type 'tree notes' Enter
Sleep 1500ms
Type 'cat notes/note.txt' Enter
Sleep 4500ms

Type "toha stage ./template ./notes2 --async | jq '{status, schema: .schema}'" Enter
Sleep 6500ms
Type "echo '"
Type '{"title":"Agent note","topic":"Research"}'
Type "' | toha continue ./notes2 -" Enter
Sleep 2800ms
Type 'toha apply ./notes2' Enter
Sleep 2500ms
Type 'cat notes2/note.txt' Enter
Sleep 3500ms
