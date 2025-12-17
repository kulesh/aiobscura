# Dashboard Redesign Plan

## Goal
Transform the TUI opening page (Live View) into a comprehensive local development environment dashboard that provides immediate situational awareness and actionable insights.

## High Priority Features (This Implementation)

### 1. "At a Glance" Summary Panel
A compact, visually appealing panel at the top showing key metrics:
- Today's activity: sessions, tokens, time spent
- Current streak (motivational)
- Active agent count
- Week comparison

### 2. Activity Heatmap
Move the 28-day activity heatmap from Projects tab to the opening page:
- Visual 4-week history using Unicode blocks (░▒▓█)
- Shows coding patterns at a glance

### 3. Quick Project Switcher
Recent projects with one-keystroke access:
- Show top 3-5 recent projects
- Press 1-5 to jump directly to project detail
- Shows session count and freshness

## Implementation Steps

### Step 1: Add Dashboard Stats to App State
- [x] Load `DashboardStats` on startup (already exists for Projects tab)
- [ ] Add recent projects list to app state
- [ ] Compute today's specific stats

### Step 2: Modify Live View Layout
Current layout:
```
[Tab Header]
[Status Bar]
[Active Sessions]
[Message Stream]
[Footer]
```

New layout:
```
[Tab Header]
[Dashboard Summary Panel]  <- NEW: At a glance + heatmap
[Quick Projects + Active Sessions side by side]  <- MODIFIED
[Message Stream]
[Footer]
```

### Step 3: Implement New Components
- `render_dashboard_summary()` - At a glance stats + heatmap
- `render_quick_projects()` - Recent projects with hotkeys
- Modify `render_active_sessions_panel()` to fit new layout

### Step 4: Add Keyboard Handling
- Add 1-9 number key handlers in Live View
- Jump to project detail on key press

## Visual Design Notes

### Color Palette (existing constants)
- Cyan: Tokens, counts
- Lime/Green: Active, success
- Gold/Yellow: Duration, time
- Purple: Special metrics
- DarkGray: Inactive, secondary

### Typography
- Bold headers
- Dim for secondary info
- Unicode symbols for visual interest (●, ▶, ░▒▓█)

## Files to Modify
- `aiobscura/src/app.rs` - Add state for dashboard data, load on startup
- `aiobscura/src/ui.rs` - New rendering functions, modify Live View layout

## Future Enhancements (Not in this PR)
- Environment health panel (agent log detection)
- Token budget tracking
- Cost estimation
- Productivity insights
