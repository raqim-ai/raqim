# ==============================================================================
# Raqim Hero Split-Terminal Recording
# ==============================================================================
Output assets/demo.gif

Set Shell "bash"
Set FontSize 13
Set Width 1500
Set Height 800
Set Theme "Catppuccin Mocha"
Set Padding 12
Set TypingSpeed 30ms

# 1. Kill any existing tmux server & launch a fresh, split session
Type "tmux kill-server 2>/dev/null; tmux new-session -s demo \; split-window -h \; select-pane -t 0" Enter
Sleep 1s

# 2. Left Pane (Pane 0): Start the pre-compiled Raqim microkernel
Type "target/release/raqim-core" Enter
# Wait 4s for WAL recovery, FastEmbed model load, and Axum port binding
Sleep 4s

# 3. Switch active focus to Right Pane (Pane 1)
Type "tmux select-pane -t 1" Enter
Sleep 500ms

# 4. Right Pane: Execute the 6-Phase Showcase Harness
Type "python raqim-py/agent_master_showcase.py"
Sleep 1s
Enter

# 5. Allow full 6-phase test to complete on screen
Sleep 18s

# 6. Clean exit from both panes to finalize recording
Type "exit" Enter
Sleep 500ms
Ctrl+C
Type "exit" Enter
Sleep 1s