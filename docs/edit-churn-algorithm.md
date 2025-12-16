# Edit Churn Detection Algorithm

This document describes how the `core.edit_churn` analytics plugin identifies
problematic file editing patterns in AI coding sessions.

## Overview

The analyzer uses a **two-pronged approach**:

1. **Statistical Outliers** - Files with significantly more edits than the session average
2. **Burst Detection** - Files with rapid consecutive edits (debugging loops)

---

## Part 1: Statistical Outlier Detection

### Problem with Fixed Thresholds

A fixed threshold (e.g., "3+ edits = high churn") doesn't account for session context:

- In a large refactoring session touching 100 files, 3 edits per file may be normal
- In a focused session touching 5 files, 3 edits to one file may be significant

### Algorithm

```
Input: file_edit_counts = {file_path: edit_count}

# Step 1: Extract counts
counts = list of all edit counts

# Step 2: Handle small samples
if len(counts) < 5:
    threshold = 3  # Fall back to simple threshold
else:
    # Step 3: Compute statistics
    median = median(counts)
    mean = mean(counts)
    stddev = standard_deviation(counts)
    
    # Step 4: Compute dynamic threshold
    # Use 2.0 standard deviations above median
    # But never go below 3 (absolute minimum for "high churn")
    threshold = max(3, median + 2.0 * stddev)

# Step 5: Filter outliers
high_churn_files = [file for file, count in file_edit_counts if count >= threshold]

# Step 6: Sort by edit count (descending)
high_churn_files.sort(by=edit_count, descending)

Output: high_churn_files, high_churn_threshold
```

### Why 2.0× Standard Deviation?

- 1.5× stddev (like box plot whiskers) catches too many files
- 2.0× stddev is more conservative, reducing false positives
- Only truly unusual files get flagged

### Examples

| Session Files | Counts | Median | StdDev | Threshold | Flagged |
|---------------|--------|--------|--------|-----------|---------|
| 5 files | [1, 1, 1, 2, 15] | 1 | 5.5 | max(3, 1+11)=12 | 15 only |
| 5 files | [5, 6, 7, 8, 9] | 7 | 1.4 | max(3, 7+2.8)=9.8 | None |
| 4 files | [1, 2, 3, 50] | - | - | 3 (fallback) | 3, 50 |
| 7 files | [1, 1, 1, 1, 1, 1, 10] | 1 | 3.2 | max(3, 1+6.4)=7.4 | 10 |

---

## Part 2: Burst Edit Detection

### Problem

Statistical outliers don't capture *how* a file was edited:

- **Deliberate iteration**: 5 edits spread over 2 hours (refining, polishing)
- **Debugging loop**: 5 edits in 3 minutes (trial-and-error fixing)

### Algorithm

```
Input: 
  - file_edit_timestamps = {file_path: [list of edit timestamps]}
  - BURST_WINDOW_SECONDS = 120  # 2 minutes

Output:
  - burst_edit_files: {file_path: burst_count}
  - burst_edit_count: total number of burst incidents

For each file with 3+ edits:
    timestamps = sorted list of edit times for this file
    burst_count = 0
    
    # Sliding window: look for 3+ edits within BURST_WINDOW
    for i in range(len(timestamps) - 2):
        window_start = timestamps[i]
        window_end = timestamps[i + 2]  # Third edit in potential burst
        
        if (window_end - window_start).seconds <= BURST_WINDOW_SECONDS:
            burst_count += 1
    
    if burst_count > 0:
        burst_edit_files[file_path] = burst_count

Output: burst_edit_files, sum(burst_edit_files.values())
```

### Examples

```
File: app.rs
Timestamps: [19:31:55, 19:32:01, 19:32:23, 19:45:00, 19:46:00]
             ↑_______ 28 seconds _______↑
                    BURST DETECTED (1 incident)

File: config.toml  
Timestamps: [10:00:00, 11:30:00, 14:00:00]
             ↑______ 1.5 hours ______↑
                    NO BURST (deliberate iteration)

File: ui.rs
Timestamps: [10:00:00, 10:00:30, 10:01:00, 10:01:30, 10:02:00]
             ↑__burst 1__↑        ↑__burst 2__↑
                    2 BURST INCIDENTS
```

### Burst Window Rationale

**2 minutes (120 seconds)** was chosen because:

- Typical edit → compile → test → fix cycle is 30-90 seconds
- 3+ edits within 2 minutes strongly suggests trial-and-error
- Allows time for reading error messages and making targeted fixes
- Short enough to distinguish from deliberate refactoring

---

## Metrics Produced

| Metric | Type | Description |
|--------|------|-------------|
| `high_churn_files` | array | Files with statistically high edit counts (outliers) |
| `high_churn_threshold` | float | The computed threshold for this session |
| `burst_edit_files` | object | Files with burst patterns: {path: burst_count} |
| `burst_edit_count` | integer | Total burst incidents across all files |

### Other Metrics (unchanged)

| Metric | Type | Description |
|--------|------|-------------|
| `edit_count` | integer | Total Edit/Write tool calls |
| `unique_files` | integer | Number of distinct files modified |
| `churn_ratio` | float | (edits - unique_files) / edits |
| `file_edit_counts` | object | Map of file path to edit count |
| `lines_added` | integer | Total lines added |
| `lines_removed` | integer | Total lines removed |
| `lines_changed` | integer | Total lines changed |
| `edits_by_extension` | object | Edits grouped by file extension |
| `first_try_files` | integer | Files edited exactly once |
| `first_try_rate` | float | Percentage of first-try files |

---

## Interpretation Guide

### High Churn Threshold

| Threshold | Session Characteristics |
|-----------|------------------------|
| 3.0 | Small session (<5 files) or low variance |
| 5.0-10.0 | Normal variance across files |
| 15.0+ | Very high variance - few files getting most attention |

### Burst Edit Count

| Count | Interpretation |
|-------|----------------|
| 0 | Clean session - deliberate, spread-out edits |
| 1-2 | Minor debugging - normal for complex features |
| 3-5 | Moderate debugging - some trial-and-error |
| 6+ | Heavy debugging - unclear requirements or difficult bugs |

### Combined Analysis

| Outliers | Bursts | Interpretation |
|----------|--------|----------------|
| Few | Few | Efficient, focused work |
| Few | Many | Rapid iteration but spread across files |
| Many | Few | Broad refactoring, deliberate |
| Many | Many | Difficult session - unclear requirements or complex bugs |

---

## Implementation Notes

### Constants

```rust
/// Minimum edit count to be considered "high churn" (absolute floor)
const HIGH_CHURN_THRESHOLD: i64 = 3;

/// Window for burst detection (seconds)
const BURST_WINDOW_SECONDS: i64 = 120;

/// Minimum files needed for statistical threshold calculation
const MIN_FILES_FOR_STATS: usize = 5;

/// Standard deviation multiplier for outlier detection
const OUTLIER_STDDEV_MULTIPLIER: f64 = 2.0;
```

### Excluded Files

The following paths are excluded from all churn analysis:

- `/.claude/plans/` - Claude Code planning documents
- `/.claude/todos/` - Claude Code todo files
- `/PLAN.md`, `/IMPLEMENTATION.md`, `/DESIGN.md`, `/ARCHITECTURE.md`

### Performance

- Statistical calculations: O(n) where n = number of unique files
- Burst detection: O(m log m) where m = edits per file (for sorting timestamps)
- Overall: Negligible for typical session sizes (< 1000 edits)

---

## Version History

- **v1** (initial): Fixed threshold of 3+ edits
- **v2** (current): Statistical outliers + burst detection
