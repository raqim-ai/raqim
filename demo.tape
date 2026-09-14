# ==============================================================================
# Raqim Hero Split-Terminal Recording (Enterprise Standard)
# ==============================================================================
Output assets/demo.gif

Set Shell "bash"
Set FontSize 15
Set Width 1500
Set Height 820
Set Theme "Catppuccin Mocha"
Set Padding 16
Set LineHeight 1.2
Set TypingSpeed 30ms

# 1. Silently initialize split session without showing tmux setup on screen
Hide
Type "tmux kill-server 2>/dev/null; sleep 0.5"
Enter
Sleep 500ms
Type "tmux new-session -d -s demo -x 150 -y 38 \; split-window -h -t demo \; select-pane -t demo:0.0"
Enter
Sleep 500ms
Type "tmux attach-session -t demo"
Enter
Sleep 500ms
Type "clear"
Enter
Sleep 500ms
Show

# 2. Left Pane: Launch Raqim Core Sovereign Daemon
Type "cargo run --release -p raqim-core"
Enter
# Wait for WAL recovery, FastEmbed model load, and Axum daemon live
Sleep 5s

# 3. Seamlessly switch focus to Right Pane
Ctrl+B
Type "o"
Sleep 1s

# 4. Right Pane: Execute Python Agent Flight Recorder Quickstart
Type "python examples/01_quickstart.py"
Enter
# Allow full 6-phase test to complete on screen
Sleep 12s

# 5. Clean, graceful exit from both panes to finalize recording
Type "exit"
Enter
Sleep 500ms
Ctrl+C
Sleep 1s
Type "exit"
Enter
Sleep 1s