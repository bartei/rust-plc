//! Debug support for the VM: breakpoints, stepping, variable inspection.

use st_ir::*;
use std::collections::HashSet;

/// Debug state managed alongside the VM.
#[derive(Debug)]
pub struct DebugState {
    /// Breakpoints: set of (func_index, instruction_index) pairs.
    breakpoints: HashSet<(u16, usize)>,
    /// Source-level breakpoints: set of byte offsets in source.
    source_breakpoints: HashSet<usize>,
    /// Current step mode.
    pub step_mode: StepMode,
    /// Call depth when step-over/step-out was initiated.
    pub step_start_depth: usize,
    /// Source offset when stepping started (to detect line changes).
    pub step_start_source_offset: usize,
    /// Source offset of the last breakpoint hit (to avoid re-triggering on same statement).
    pub last_breakpoint_offset: usize,
    /// Whether the VM is currently paused.
    pub paused: bool,
    /// Reason for the last pause.
    pub pause_reason: PauseReason,
}

/// How the debugger should advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMode {
    /// Run freely until a breakpoint or pause request.
    Continue,
    /// Stop at the next instruction (any depth).
    StepIn,
    /// Stop at the next instruction at the same or lower call depth.
    StepOver,
    /// Stop when returning to a lower call depth.
    StepOut,
    /// VM is paused — don't execute.
    Paused,
}

/// Why the VM paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    /// Hit a breakpoint.
    Breakpoint,
    /// Completed a step operation.
    Step,
    /// User requested pause.
    PauseRequest,
    /// Program entry (stopped on launch).
    Entry,
    /// Not paused.
    None,
}

impl Default for DebugState {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugState {
    pub fn new() -> Self {
        Self {
            breakpoints: HashSet::new(),
            source_breakpoints: HashSet::new(),
            step_mode: StepMode::Continue,
            step_start_depth: 0,
            step_start_source_offset: 0,
            last_breakpoint_offset: 0,
            paused: false,
            pause_reason: PauseReason::None,
        }
    }

    /// Set a breakpoint at a source byte offset.
    pub fn set_source_breakpoint(&mut self, byte_offset: usize) {
        self.source_breakpoints.insert(byte_offset);
    }

    /// Remove a breakpoint at a source byte offset.
    pub fn remove_source_breakpoint(&mut self, byte_offset: usize) {
        self.source_breakpoints.remove(&byte_offset);
    }

    /// Clear all breakpoints.
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
        self.source_breakpoints.clear();
    }

    /// Set breakpoints from source line numbers by resolving via source map.
    /// Returns the actual byte offsets where breakpoints were set.
    pub fn set_line_breakpoints(
        &mut self,
        module: &Module,
        source: &str,
        lines: &[u32],
    ) -> Vec<Option<usize>> {
        // Build line → byte offset mapping
        let line_offsets = compute_line_offsets(source);

        lines
            .iter()
            .map(|&line| {
                // DAP lines are 1-indexed, line_offsets is 0-indexed
                let line_idx = (line as usize).saturating_sub(1);
                let line_start = line_offsets.get(line_idx).copied()?;
                let line_end = line_offsets
                    .get(line_idx + 1)
                    .copied()
                    .unwrap_or(source.len());

                // Find any instruction whose source map overlaps this line
                for func in &module.functions {
                    for sm in &func.source_map {
                        if sm.byte_offset >= line_start
                            && sm.byte_offset < line_end
                            && sm.byte_offset > 0
                        {
                            self.source_breakpoints.insert(sm.byte_offset);
                            return Some(sm.byte_offset);
                        }
                    }
                }
                None
            })
            .collect()
    }

    /// Check if the current instruction should cause a pause.
    /// Called before each instruction in the VM.
    pub fn should_pause(
        &mut self,
        func_index: u16,
        pc: usize,
        call_depth: usize,
        source_map: &[SourceLocation],
    ) -> Option<PauseReason> {
        // Get current instruction's source offset
        let current_source = source_map
            .get(pc)
            .map(|sm| sm.byte_offset)
            .unwrap_or(0);

        // Clear breakpoint suppression when we move to a different source location
        if current_source > 0 && current_source != self.last_breakpoint_offset {
            self.last_breakpoint_offset = 0;
        }

        match self.step_mode {
            StepMode::Paused => Some(PauseReason::PauseRequest),
            StepMode::StepIn => {
                // Stop at next instruction with a different source location
                if current_source > 0 && current_source != self.step_start_source_offset {
                    Some(PauseReason::Step)
                } else if current_source == 0 {
                    // No source info — skip this instruction silently
                    None
                } else {
                    // Same source location — keep going
                    None
                }
            }
            StepMode::StepOver => {
                if call_depth <= self.step_start_depth
                    && current_source > 0
                    && current_source != self.step_start_source_offset
                {
                    Some(PauseReason::Step)
                } else {
                    self.check_breakpoint(func_index, pc, source_map)
                }
            }
            StepMode::StepOut => {
                if call_depth < self.step_start_depth {
                    Some(PauseReason::Step)
                } else {
                    self.check_breakpoint(func_index, pc, source_map)
                }
            }
            StepMode::Continue => self.check_breakpoint(func_index, pc, source_map),
        }
    }

    fn check_breakpoint(
        &mut self,
        func_index: u16,
        pc: usize,
        source_map: &[SourceLocation],
    ) -> Option<PauseReason> {
        // Check instruction-level breakpoints
        if self.breakpoints.contains(&(func_index, pc)) {
            return Some(PauseReason::Breakpoint);
        }
        // Check source-level breakpoints
        if let Some(sm) = source_map.get(pc) {
            if sm.byte_offset > 0
                && self.source_breakpoints.contains(&sm.byte_offset)
                && sm.byte_offset != self.last_breakpoint_offset
            {
                self.last_breakpoint_offset = sm.byte_offset;
                return Some(PauseReason::Breakpoint);
            }
        }
        None
    }

    /// Pause the VM.
    pub fn pause(&mut self) {
        self.step_mode = StepMode::Paused;
        self.paused = true;
        self.pause_reason = PauseReason::PauseRequest;
    }

    /// Resume execution with the given step mode.
    pub fn resume(&mut self, mode: StepMode, current_depth: usize) {
        self.step_mode = mode;
        self.step_start_depth = current_depth;
        self.paused = false;
        self.pause_reason = PauseReason::None;
    }

    /// Resume with source offset tracking (for line-based stepping).
    pub fn resume_with_source(&mut self, mode: StepMode, current_depth: usize, source_offset: usize) {
        self.step_mode = mode;
        self.step_start_depth = current_depth;
        self.step_start_source_offset = source_offset;
        // Don't clear last_breakpoint_offset here — it prevents re-triggering
        // at the same instruction. It gets cleared naturally when the VM
        // advances past the breakpoint's source offset.
        self.paused = false;
        self.pause_reason = PauseReason::None;
    }

    /// Record that the VM paused.
    pub fn mark_paused(&mut self, reason: PauseReason) {
        self.paused = true;
        self.pause_reason = reason;
        self.step_mode = StepMode::Paused;
    }
}

/// A snapshot of a call frame for the debugger.
#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub func_index: u16,
    pub func_name: String,
    pub pc: usize,
    pub source_offset: usize,
    pub source_end: usize,
}

/// A variable visible in the debugger.
#[derive(Debug, Clone)]
pub struct VariableInfo {
    pub name: String,
    pub value: String,
    pub ty: String,
    pub var_ref: u32,
}

/// Format a Value for display in the debugger.
pub fn format_value(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Real(r) => format!("{r:.6}"),
        Value::String(s) => format!("'{s}'"),
        Value::Time(ms) => {
            if *ms >= 60_000 {
                let min = ms / 60_000;
                let sec = (ms % 60_000) / 1000;
                let rem_ms = ms % 1000;
                if rem_ms > 0 {
                    format!("T#{min}m{sec}s{rem_ms}ms")
                } else if sec > 0 {
                    format!("T#{min}m{sec}s")
                } else {
                    format!("T#{min}m")
                }
            } else if *ms >= 1000 {
                let sec = ms / 1000;
                let rem_ms = ms % 1000;
                if rem_ms > 0 {
                    format!("T#{sec}s{rem_ms}ms")
                } else {
                    format!("T#{sec}s")
                }
            } else {
                format!("T#{ms}ms")
            }
        }
        Value::Ref(scope, slot) => format!("REF({scope}:{slot})"),
        Value::Null => "NULL".to_string(),
        Value::Void => "VOID".to_string(),
    }
}

/// Format a VarType for display.
pub fn format_var_type(ty: VarType) -> &'static str {
    match ty {
        VarType::Bool => "BOOL",
        VarType::Int => "INT",
        VarType::UInt => "UINT",
        VarType::Real => "REAL",
        VarType::String => "STRING",
        VarType::Time => "TIME",
        VarType::FbInstance(_) => "FB_INSTANCE",
        VarType::Ref => "REF_TO",
    }
}

/// Compute byte offsets for each line (0-indexed).
fn compute_line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0]; // line 0 starts at byte 0
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_offsets() {
        let offsets = compute_line_offsets("abc\ndef\nghi\n");
        assert_eq!(offsets, vec![0, 4, 8, 12]);
    }

    #[test]
    fn test_debug_state_breakpoints() {
        let mut ds = DebugState::new();
        ds.set_source_breakpoint(100);
        ds.set_source_breakpoint(200);

        let sm = vec![
            SourceLocation { byte_offset: 50, byte_end: 60 },
            SourceLocation { byte_offset: 100, byte_end: 110 },
            SourceLocation { byte_offset: 150, byte_end: 160 },
        ];

        // PC 0 → offset 50, no breakpoint
        assert_eq!(ds.should_pause(0, 0, 0, &sm), None);
        // PC 1 → offset 100, breakpoint!
        assert_eq!(ds.should_pause(0, 1, 0, &sm), Some(PauseReason::Breakpoint));
        // PC 2 → offset 150, no breakpoint
        assert_eq!(ds.should_pause(0, 2, 0, &sm), None);
    }

    #[test]
    fn test_step_modes() {
        let mut ds = DebugState::new();
        let sm = vec![SourceLocation { byte_offset: 10, byte_end: 20 }];

        // StepIn: always pause
        ds.step_mode = StepMode::StepIn;
        assert_eq!(ds.should_pause(0, 0, 0, &sm), Some(PauseReason::Step));

        // StepOver at depth 1: don't pause at depth 2
        ds.step_mode = StepMode::StepOver;
        ds.step_start_depth = 1;
        assert_eq!(ds.should_pause(0, 0, 2, &sm), None);
        // Pause at depth 1
        assert_eq!(ds.should_pause(0, 0, 1, &sm), Some(PauseReason::Step));
        // Pause at depth 0
        assert_eq!(ds.should_pause(0, 0, 0, &sm), Some(PauseReason::Step));

        // StepOut from depth 2: don't pause at depth 2
        ds.step_mode = StepMode::StepOut;
        ds.step_start_depth = 2;
        assert_eq!(ds.should_pause(0, 0, 2, &sm), None);
        // Pause at depth 1
        assert_eq!(ds.should_pause(0, 0, 1, &sm), Some(PauseReason::Step));
    }

    #[test]
    fn test_pause_resume() {
        let mut ds = DebugState::new();
        assert!(!ds.paused);

        ds.pause();
        assert!(ds.paused);
        assert_eq!(ds.pause_reason, PauseReason::PauseRequest);

        ds.resume(StepMode::Continue, 0);
        assert!(!ds.paused);
        assert_eq!(ds.step_mode, StepMode::Continue);
    }

    #[test]
    fn test_clear_breakpoints() {
        let mut ds = DebugState::new();
        ds.set_source_breakpoint(100);
        ds.set_source_breakpoint(200);
        ds.clear_breakpoints();

        let sm = vec![SourceLocation { byte_offset: 100, byte_end: 110 }];
        assert_eq!(ds.should_pause(0, 0, 0, &sm), None);
    }

    #[test]
    fn test_remove_breakpoint() {
        let mut ds = DebugState::new();
        ds.set_source_breakpoint(100);
        ds.remove_source_breakpoint(100);

        let sm = vec![SourceLocation { byte_offset: 100, byte_end: 110 }];
        assert_eq!(ds.should_pause(0, 0, 0, &sm), None);
    }

    #[test]
    fn test_format_value() {
        assert_eq!(format_value(&Value::Bool(true)), "TRUE");
        assert_eq!(format_value(&Value::Bool(false)), "FALSE");
        assert_eq!(format_value(&Value::Int(42)), "42");
        assert_eq!(format_value(&Value::UInt(100)), "100");
        assert_eq!(format_value(&Value::Real(1.5)), "1.500000");
        assert_eq!(format_value(&Value::String("hello".into())), "'hello'");
        assert_eq!(format_value(&Value::Time(5000)), "T#5s");
        assert_eq!(format_value(&Value::Time(100)), "T#100ms");
        assert_eq!(format_value(&Value::Time(65000)), "T#1m5s");
        assert_eq!(format_value(&Value::Time(61500)), "T#1m1s500ms");
        assert_eq!(format_value(&Value::Void), "VOID");
    }

    #[test]
    fn test_format_var_type() {
        assert_eq!(format_var_type(VarType::Bool), "BOOL");
        assert_eq!(format_var_type(VarType::Int), "INT");
        assert_eq!(format_var_type(VarType::Real), "REAL");
        assert_eq!(format_var_type(VarType::String), "STRING");
        assert_eq!(format_var_type(VarType::Time), "TIME");
        assert_eq!(format_var_type(VarType::FbInstance(0)), "FB_INSTANCE");
    }

    #[test]
    fn test_set_line_breakpoints() {
        let module = Module {
            functions: vec![Function {
                name: "Main".into(),
                kind: PouKind::Program,
                register_count: 1,
                instructions: vec![
                    Instruction::Nop,
                    Instruction::Nop,
                    Instruction::Nop,
                ],
                label_positions: vec![],
                locals: MemoryLayout::default(),
                source_map: vec![
                    SourceLocation { byte_offset: 0, byte_end: 5 },
                    SourceLocation { byte_offset: 14, byte_end: 20 },   // line 1
                    SourceLocation { byte_offset: 28, byte_end: 35 },   // line 2
                ],
                body_start_pc: 0,
            }],
            globals: MemoryLayout::default(),
            type_defs: vec![],
        };
        let source = "PROGRAM Main\n    x := 1;\n    x := 2;\nEND_PROGRAM\n";

        let mut ds = DebugState::new();
        // DAP uses 1-indexed lines: line 2 = "    x := 1;", line 3 = "    x := 2;"
        let results = ds.set_line_breakpoints(&module, source, &[2, 3, 99]);
        // Line 2 should map, line 3 should map, line 99 should not
        assert!(results[0].is_some(), "Line 2 should map to an instruction");
        assert!(results[1].is_some(), "Line 3 should map to an instruction");
        assert!(results[2].is_none(), "Line 99 should not map");
    }
}
