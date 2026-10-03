# ---
# relationships:
#   demonstrates: overview
#   references: interview-protocol
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
Set TypingSpeed 32ms

Hide
Type 'unset NO_COLOR; export LANG=C.UTF-8 TERM=xterm-256color COLORTERM=truecolor LS_COLORS="di=01;34:ex=01;32"' Enter
Type 'export PATH="$PWD/target/debug:$PATH"' Enter
Type 'export PS1="demo> "' Enter
Type 'DEMO_ROOT=$(mktemp -d); cd "$DEMO_ROOT"' Enter
Type 'export HOME="$DEMO_ROOT/home" XDG_CONFIG_HOME="$DEMO_ROOT/config" XDG_DATA_HOME="$DEMO_ROOT/data" XDG_CACHE_HOME="$DEMO_ROOT/cache" XDG_STATE_HOME="$DEMO_ROOT/state"' Enter
Type 'mkdir -p "$HOME"' Enter
Type 'clear' Enter
Show

Sleep 700ms
Type 'toha apply toha-demo ./notes' Enter
Sleep 800ms
Type 'Sample note' Enter
Sleep 400ms
Type 'Planning' Enter
Sleep 2000ms
Type 'tree notes' Enter
Sleep 2000ms
Type 'cat notes/note.txt' Enter
Sleep 6000ms

Type "toha stage toha-demo ./notes2 --async | jq '{status, schema: .schema}'" Enter
Sleep 8700ms
Type "echo '"
Type '{"title":"Agent note","topic":"Research"}'
Type "' | toha continue ./notes2 -" Enter
Sleep 3700ms
Type 'toha apply ./notes2' Enter
Sleep 3300ms
Type 'cat notes2/note.txt' Enter
Sleep 4700ms
