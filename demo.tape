# ==============================================================================
# Raqim Hero Terminal Tape (Split-Pane)
# ==============================================================================
Output assets/demo.gif

Set Shell "bash"
Set FontSize 14
Set Width 1400
Set Height 750
Set Theme "Catppuccin Mocha"
Set Padding 10
Set TypingSpeed 25ms

# Step 1: Initialize tmux and create vertical split
Type "tmux -u new-session -d -s raqim_demo" Enter
Sleep 500ms
Type "tmux split-window -h -t raqim_demo" Enter
Sleep 500ms

# Step 2: Launch Daemon in Pane 0 (Left)
Type "tmux select-pane -t raqim_demo:0.0" Enter
Type "tmux send-keys -t raqim_demo:0.0 'cargo run --release --bin raqim-core' C-m" Enter

# Step 3: Attach view to tmux session
Type "tmux -u attach-session -t raqim_demo" Enter
Sleep 4s

# Step 4: Focus Pane 1 (Right) and run the showcase
Type "python raqim-py/agent_master_showcase.py"
Sleep 1s
Enter

# Step 5: Allow full 6-phase test to complete
Sleep 18s

# Step 6: Hold final frame and exit cleanly
Sleep 3s
Ctrl+C
Type "exit" Enter