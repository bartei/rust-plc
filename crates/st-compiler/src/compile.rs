//! Compiler: AST → IR module.

use st_ir::*;
use st_syntax::ast::*;

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("undeclared variable '{0}'")]
    UndeclaredVariable(String),
    #[error("undeclared function '{0}'")]
    UndeclaredFunction(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// Compile a parsed source file into an IR module.
pub fn compile(source_file: &SourceFile) -> Result<Module, CompileError> {
    compile_with_native_fbs(source_file, None)
}

/// Compile a parsed source file into an IR module, optionally injecting
/// native FB types from a registry.
///
/// When a [`NativeFbRegistry`] is provided, synthetic [`Function`] entries are
/// created for each registered native FB type (with correct `MemoryLayout` but
/// empty instruction bodies). These entries are registered BEFORE Pass 1 so
/// that user code declaring native FB instances resolves correctly.
pub fn compile_with_native_fbs(
    source_file: &SourceFile,
    registry: Option<&st_comm_api::NativeFbRegistry>,
) -> Result<Module, CompileError> {
    let mut ctx = ModuleCompiler {
        functions: Vec::new(),
        globals: MemoryLayout::default(),
        type_defs: Vec::new(),
        class_bases: std::collections::HashMap::new(),
        class_var_blocks: std::collections::HashMap::new(),
        pending_global_inits: Vec::new(),
    };

    // Pre-pass: inject synthetic Function entries for native FBs.
    // These must come before Pass 1 so that when the compiler encounters
    // `VAR dev : NativeFbType; END_VAR`, it finds the Function entry and
    // correctly sets VarType::FbInstance(func_idx) on the local slot.
    let mut native_fb_indices = Vec::new();
    if let Some(reg) = registry {
        for native_fb in reg.all() {
            let layout = native_fb.layout();
            let td_base = ctx.type_defs.len() as u16;
            let mem_layout = st_comm_api::layout_to_memory_layout(layout, &mut ctx.type_defs, td_base);
            let func_idx = ctx.functions.len() as u16;
            ctx.functions.push(Function {
                name: layout.type_name.clone(),
                kind: PouKind::FunctionBlock,
                register_count: 0,
                instructions: vec![],
                label_positions: vec![],
                locals: mem_layout,
                source_map: vec![],
                body_start_pc: 0,
            });
            native_fb_indices.push(func_idx);
        }
    }

    // Pass 1: register all POUs so cross-references work
    for item in &source_file.items {
        ctx.register_item(item);
    }
    // Pass 2: compile bodies. FUNCTIONs, FUNCTION_BLOCKs, and CLASSes
    // must be compiled BEFORE PROGRAMs so that field-access resolution
    // (e.g., `filler.fill_count`) can look up the callee's locals layout.
    // Without this ordering, a PROGRAM compiled before its callee's FB
    // would see empty locals and silently resolve all fields to index 0.
    for item in &source_file.items {
        if !matches!(item, TopLevelItem::Program(_)) {
            ctx.compile_item(item)?;
        }
    }
    for item in &source_file.items {
        if matches!(item, TopLevelItem::Program(_)) {
            ctx.compile_item(item)?;
        }
    }
    // Pass 3: synthesize the global initializer function. Runs once at
    // engine startup to apply `VAR_GLOBAL x : T := <expr>;` initial values
    // (without it, globals would be left at `Value::default_for_type`,
    // which is 0/false/empty for everything).
    ctx.compile_global_init();
    Ok(Module {
        functions: ctx.functions,
        globals: ctx.globals,
        type_defs: ctx.type_defs,
        native_fb_indices,
    })
}

/// Synthetic function name for the global variable initializer. The VM's
/// `run_global_init()` looks this up by name; if it doesn't exist (e.g.
/// modules compiled before this feature, or modules with no global
/// initializers) the call is a no-op.
pub const GLOBAL_INIT_FUNCTION_NAME: &str = "__global_init";

struct ModuleCompiler {
    functions: Vec<Function>,
    globals: MemoryLayout,
    type_defs: Vec<TypeDef>,
    /// Maps class name → base class name (for inheritance chain resolution).
    class_bases: std::collections::HashMap<String, String>,
    /// Maps class name → var_blocks (for inherited var access in methods).
    class_var_blocks: std::collections::HashMap<String, Vec<VarBlock>>,
    /// Pending global variable initializers collected during pass 1.
    /// Each entry is (slot index in `globals`, init expression). After pass
    /// 2 finishes, we synthesize a `__global_init` function containing one
    /// `StoreGlobal` per entry; the engine calls this once at startup so
    /// `VAR_GLOBAL counter : USINT := 250;` actually applies its 250.
    pending_global_inits: Vec<(u16, Expression)>,
}

impl ModuleCompiler {
    fn register_item(&mut self, item: &TopLevelItem) {
        match item {
            TopLevelItem::Program(p) => {
                self.functions.push(Function {
                    name: p.name.name.clone(),
                    kind: PouKind::Program,
                    register_count: 0,
                    instructions: Vec::new(),
                    label_positions: Vec::new(),
                    locals: MemoryLayout::default(),
                    source_map: Vec::new(),
                    body_start_pc: 0,
                });
            }
            TopLevelItem::Function(f) => {
                self.functions.push(Function {
                    name: f.name.name.clone(),
                    kind: PouKind::Function,
                    register_count: 0,
                    instructions: Vec::new(),
                    label_positions: Vec::new(),
                    locals: MemoryLayout::default(),
                    source_map: Vec::new(),
                    body_start_pc: 0,
                });
            }
            TopLevelItem::FunctionBlock(fb) => {
                self.functions.push(Function {
                    name: fb.name.name.clone(),
                    kind: PouKind::FunctionBlock,
                    register_count: 0,
                    instructions: Vec::new(),
                    label_positions: Vec::new(),
                    locals: MemoryLayout::default(),
                    source_map: Vec::new(),
                    body_start_pc: 0,
                });
            }
            TopLevelItem::GlobalVarDeclaration(vb) => {
                for decl in &vb.declarations {
                    let ty = Self::var_type_from_ast(&decl.ty);
                    let int_width = Self::int_width_from_ast(&decl.ty);
                    for name in &decl.names {
                        let offset = self.globals.total_size();
                        let size = ty.size();
                        let slot_idx = self.globals.slots.len() as u16;
                        self.globals.slots.push(VarSlot {
                            name: name.name.clone(),
                            ty,
                            offset,
                            size,
                            retain: vb.qualifiers.contains(&VarQualifier::Retain),
                            persistent: vb.qualifiers.contains(&VarQualifier::Persistent),
                            int_width,
                        });
                        // Defer initializer compilation until pass 3 — we
                        // need all functions registered first so the init
                        // expressions can call them (e.g. INT_TO_REAL).
                        if let Some(init_expr) = &decl.initial_value {
                            self.pending_global_inits
                                .push((slot_idx, init_expr.clone()));
                        }
                    }
                }
            }
            TopLevelItem::Class(cls) => {
                // Record inheritance mapping
                if let Some(ref base) = cls.base_class {
                    self.class_bases.insert(
                        cls.name.name.to_uppercase(),
                        base.to_uppercase(),
                    );
                }
                // Store var_blocks for inherited var access
                self.class_var_blocks.insert(
                    cls.name.name.to_uppercase(),
                    cls.var_blocks.clone(),
                );
                // Register the class itself as a "function block"-like entry
                self.functions.push(Function {
                    name: cls.name.name.clone(),
                    kind: PouKind::Class,
                    register_count: 0,
                    instructions: Vec::new(),
                    label_positions: Vec::new(),
                    locals: MemoryLayout::default(),
                    source_map: Vec::new(),
                    body_start_pc: 0,
                });
                // Register each method as a separate function: ClassName.MethodName
                for method in &cls.methods {
                    if !method.is_abstract {
                        self.functions.push(Function {
                            name: format!("{}.{}", cls.name.name, method.name.name),
                            kind: PouKind::Method,
                            register_count: 0,
                            instructions: Vec::new(),
                            label_positions: Vec::new(),
                            locals: MemoryLayout::default(),
                            source_map: Vec::new(),
                            body_start_pc: 0,
                        });
                    }
                }
            }
            TopLevelItem::Interface(_) => {
                // Interfaces have no runtime representation
            }
            TopLevelItem::TypeDeclaration(tdb) => {
                for tdef in &tdb.definitions {
                    if let TypeDefKind::Struct(st) = &tdef.ty {
                        let fields: Vec<VarSlot> = st.fields.iter().enumerate().map(|(i, f)| {
                            let ty = Self::var_type_from_ast(&f.ty);
                            let int_width = Self::int_width_from_ast(&f.ty);
                            let size = ty.size();
                            VarSlot {
                                name: f.name.name.clone(),
                                ty,
                                offset: i,
                                size,
                                retain: false,
                                persistent: false,
                                int_width,
                            }
                        }).collect();
                        self.type_defs.push(TypeDef::Struct {
                            name: tdef.name.name.clone(),
                            fields,
                        });
                    }
                }
            }
        }
    }

    fn compile_item(&mut self, item: &TopLevelItem) -> Result<(), CompileError> {
        match item {
            TopLevelItem::Program(p) => {
                let func_idx = self.find_func(&p.name.name)?;
                let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases, &mut self.type_defs);
                fc.compile_var_blocks(&p.var_blocks);
                let body_start_pc = fc.current_pc();
                fc.compile_statements(&p.body)?;
                fc.emit(Instruction::RetVoid);
                self.functions[func_idx] = fc.finish(
                    p.name.name.clone(),
                    PouKind::Program,
                    body_start_pc,
                );
            }
            TopLevelItem::Function(f) => {
                let func_idx = self.find_func(&f.name.name)?;
                let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases, &mut self.type_defs);
                fc.compile_var_blocks(&f.var_blocks);
                let ret_ty = Self::var_type_from_ast(&f.return_type);
                let ret_width = Self::int_width_from_ast(&f.return_type);
                let ret_slot = fc.add_local(&f.name.name, ret_ty, ret_width);
                let body_start_pc = fc.current_pc();
                fc.compile_statements(&f.body)?;
                let ret_reg = fc.alloc_reg();
                fc.emit(Instruction::LoadLocal(ret_reg, ret_slot));
                fc.emit(Instruction::Ret(ret_reg));
                self.functions[func_idx] = fc.finish(
                    f.name.name.clone(),
                    PouKind::Function,
                    body_start_pc,
                );
            }
            TopLevelItem::FunctionBlock(fb) => {
                let func_idx = self.find_func(&fb.name.name)?;
                let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases, &mut self.type_defs);
                fc.compile_var_blocks(&fb.var_blocks);
                let body_start_pc = fc.current_pc();
                fc.compile_statements(&fb.body)?;
                fc.emit(Instruction::RetVoid);
                self.functions[func_idx] = fc.finish(
                    fb.name.name.clone(),
                    PouKind::FunctionBlock,
                    body_start_pc,
                );
            }
            TopLevelItem::Class(cls) => {
                // Compile the class body (inherited + own var initializers)
                let func_idx = self.find_func(&cls.name.name)?;
                // Collect inherited var blocks before borrowing type_defs mutably
                let inherited_for_class = self.collect_inherited_var_blocks(&cls.name.name);
                let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases, &mut self.type_defs);
                for vb in &inherited_for_class {
                    fc.compile_var_blocks(std::slice::from_ref(vb));
                }
                fc.compile_var_blocks(&cls.var_blocks);
                let body_start_pc = fc.current_pc();
                fc.emit(Instruction::RetVoid);
                self.functions[func_idx] = fc.finish(
                    cls.name.name.clone(),
                    PouKind::Class,
                    body_start_pc,
                );

                // Compile each method — class vars (including inherited) are compiled
                // first so methods can access all fields in the hierarchy.
                let inherited_var_blocks = self.collect_inherited_var_blocks(&cls.name.name);
                for method in &cls.methods {
                    if method.is_abstract {
                        continue;
                    }
                    let method_name = format!("{}.{}", cls.name.name, method.name.name);
                    let method_idx = self.find_func(&method_name)?;
                    let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases, &mut self.type_defs);
                    // First: register inherited var_blocks (ancestor classes)
                    for vb in &inherited_var_blocks {
                        fc.register_var_blocks(std::slice::from_ref(vb));
                    }
                    // Then: register this class's var_blocks (no init — state from instance)
                    fc.register_var_blocks(&cls.var_blocks);
                    // Then: add method's own var_blocks (with init)
                    fc.compile_var_blocks(&method.var_blocks);
                    // Define return variable if method has return type
                    let ret_slot = method.return_type.as_ref().map(|dt| {
                        let ret_ty = Self::var_type_from_ast(dt);
                        let ret_width = Self::int_width_from_ast(dt);
                        fc.add_local(&method.name.name, ret_ty, ret_width)
                    });
                    let body_start_pc = fc.current_pc();
                    fc.compile_statements(&method.body)?;
                    if let Some(slot) = ret_slot {
                        let ret_reg = fc.alloc_reg();
                        fc.emit(Instruction::LoadLocal(ret_reg, slot));
                        fc.emit(Instruction::Ret(ret_reg));
                    } else {
                        fc.emit(Instruction::RetVoid);
                    }
                    self.functions[method_idx] = fc.finish(
                        method_name,
                        PouKind::Method,
                        body_start_pc,
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Synthesize the `__global_init` function from the deferred global
    /// initializers collected during pass 1. Each entry compiles to a
    /// `<expr>` → register → `StoreGlobal(slot, reg)` sequence. Skipped
    /// entirely if no globals had initializers (no synthetic function
    /// is added to the module — the VM's run_global_init becomes a no-op).
    fn compile_global_init(&mut self) {
        if self.pending_global_inits.is_empty() {
            return;
        }
        let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases, &mut self.type_defs);
        // Drain the pending list into a local so we can iterate without
        // holding the borrow on self.
        let pending: Vec<(u16, Expression)> =
            std::mem::take(&mut self.pending_global_inits);
        for (slot, expr) in &pending {
            fc.set_source(expr.range());
            let reg = fc.compile_expression(expr);
            fc.emit(Instruction::StoreGlobal(*slot, reg));
        }
        fc.emit(Instruction::RetVoid);
        let func = fc.finish(
            GLOBAL_INIT_FUNCTION_NAME.to_string(),
            PouKind::Function,
            0,
        );
        self.functions.push(func);
    }

    /// Collect var_blocks from all ancestor classes, root-first.
    fn collect_inherited_var_blocks(&self, class_name: &str) -> Vec<VarBlock> {
        let mut chain = Vec::new();
        let mut current = self.class_bases.get(&class_name.to_uppercase()).cloned();
        while let Some(ref parent) = current {
            if let Some(vbs) = self.class_var_blocks.get(parent) {
                chain.push(vbs.clone());
            }
            current = self.class_bases.get(parent).cloned();
        }
        // Reverse: root ancestor first
        chain.reverse();
        chain.into_iter().flatten().collect()
    }

    fn find_func(&self, name: &str) -> Result<usize, CompileError> {
        self.functions
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| CompileError::Internal(format!("function '{name}' not registered")))
    }

    fn var_type_from_ast(dt: &DataType) -> VarType {
        match dt {
            DataType::Elementary(e) => match e {
                ElementaryType::Bool => VarType::Bool,
                ElementaryType::Sint | ElementaryType::Int | ElementaryType::Dint
                | ElementaryType::Lint => VarType::Int,
                ElementaryType::Usint | ElementaryType::Uint | ElementaryType::Udint
                | ElementaryType::Ulint => VarType::UInt,
                ElementaryType::Real | ElementaryType::Lreal => VarType::Real,
                ElementaryType::Byte | ElementaryType::Word | ElementaryType::Dword
                | ElementaryType::Lword => VarType::UInt,
                ElementaryType::Time | ElementaryType::Ltime => VarType::Time,
                ElementaryType::Date | ElementaryType::Ldate | ElementaryType::Tod
                | ElementaryType::Ltod | ElementaryType::Dt | ElementaryType::Ldt => VarType::Time,
            },
            DataType::String(_) => VarType::String,
            DataType::Ref(_) => VarType::Ref,
            DataType::Array(_) => VarType::Int, // placeholder: resolved in compile_var_blocks_inner
            DataType::UserDefined(_) => VarType::Int, // simplified: resolved at link time
        }
    }

    /// Original integer width / signedness for a source AST type. The VM
    /// uses this at store time to wrap values to the declared bit width
    /// (so a SINT cycle counter wraps at 127→-128 instead of growing to
    /// the i64 range).
    fn int_width_from_ast(dt: &DataType) -> IntWidth {
        match dt {
            DataType::Elementary(e) => match e {
                ElementaryType::Sint => IntWidth::I8,
                ElementaryType::Usint | ElementaryType::Byte => IntWidth::U8,
                ElementaryType::Int => IntWidth::I16,
                ElementaryType::Uint | ElementaryType::Word => IntWidth::U16,
                ElementaryType::Dint => IntWidth::I32,
                ElementaryType::Udint | ElementaryType::Dword => IntWidth::U32,
                ElementaryType::Lint => IntWidth::I64,
                ElementaryType::Ulint | ElementaryType::Lword => IntWidth::U64,
                _ => IntWidth::None,
            },
            _ => IntWidth::None,
        }
    }
}

// =============================================================================
// Function-level compiler
// =============================================================================

struct FunctionCompiler<'a> {
    instructions: Vec<Instruction>,
    source_map: Vec<SourceLocation>,
    locals: MemoryLayout,
    next_reg: u16,
    next_label: u32,
    label_positions: Vec<usize>,
    /// Reference to all module functions (for cross-function calls).
    module_functions: &'a [Function],
    /// Reference to global variables.
    globals: &'a MemoryLayout,
    /// Mutable reference to user-defined type definitions (for struct field
    /// resolution and array type creation).
    type_defs: &'a mut Vec<TypeDef>,
    /// Loop exit label stack (for EXIT statements).
    loop_exit_labels: Vec<Label>,
    /// Source range to attach to next emitted instruction.
    pending_source: Option<TextRange>,
    /// Maps local slot index → FB type name (for resolving FB instance calls).
    fb_type_names: std::collections::HashMap<u16, String>,
    /// Maps class name (uppercase) → base class name (uppercase) for inheritance.
    class_bases: &'a std::collections::HashMap<String, String>,
}

impl<'a> FunctionCompiler<'a> {
    fn new(
        module_functions: &'a [Function],
        globals: &'a MemoryLayout,
        class_bases: &'a std::collections::HashMap<String, String>,
        type_defs: &'a mut Vec<TypeDef>,
    ) -> Self {
        Self {
            instructions: Vec::new(),
            source_map: Vec::new(),
            locals: MemoryLayout::default(),
            next_reg: 0,
            next_label: 0,
            label_positions: Vec::new(),
            module_functions,
            globals,
            type_defs,
            loop_exit_labels: Vec::new(),
            pending_source: None,
            fb_type_names: std::collections::HashMap::new(),
            class_bases,
        }
    }

    fn current_pc(&self) -> usize {
        self.instructions.len()
    }

    fn alloc_reg(&mut self) -> Reg {
        let r = self.next_reg;
        self.next_reg += 1;
        r
    }

    fn alloc_label(&mut self) -> Label {
        let l = self.next_label;
        self.next_label += 1;
        // Ensure label_positions is big enough
        if self.label_positions.len() <= l as usize {
            self.label_positions
                .resize(l as usize + 1, usize::MAX);
        }
        l
    }

    fn place_label(&mut self, label: Label) {
        let pos = self.instructions.len();
        if self.label_positions.len() <= label as usize {
            self.label_positions
                .resize(label as usize + 1, usize::MAX);
        }
        self.label_positions[label as usize] = pos;
    }

    fn emit(&mut self, instr: Instruction) {
        if let Some(range) = self.pending_source.take() {
            self.source_map.push(SourceLocation {
                byte_offset: range.start,
                byte_end: range.end,
            });
        } else {
            self.source_map.push(SourceLocation::default());
        }
        self.instructions.push(instr);
    }

    fn emit_sourced(&mut self, instr: Instruction, range: TextRange) {
        self.pending_source = None; // explicit source overrides pending
        self.source_map.push(SourceLocation {
            byte_offset: range.start,
            byte_end: range.end,
        });
        self.instructions.push(instr);
    }

    /// Set the source range for the next emitted instruction.
    fn set_source(&mut self, range: TextRange) {
        self.pending_source = Some(range);
    }

    fn add_local(&mut self, name: &str, ty: VarType, int_width: IntWidth) -> u16 {
        self.add_local_with_qualifiers(name, ty, int_width, false, false)
    }

    /// Simple constant evaluation for integer literal expressions (array bounds).
    fn const_eval_int_expr(expr: &Expression) -> Option<i64> {
        match expr {
            Expression::Literal(lit) => match &lit.kind {
                LiteralKind::Integer(n) => Some(*n),
                LiteralKind::Bool(true) => Some(1),
                LiteralKind::Bool(false) => Some(0),
                _ => None,
            },
            Expression::Unary(u) if matches!(u.op, UnaryOp::Neg) => {
                Self::const_eval_int_expr(&u.operand).map(|v| -v)
            }
            _ => None,
        }
    }

    fn add_local_with_qualifiers(
        &mut self,
        name: &str,
        ty: VarType,
        int_width: IntWidth,
        retain: bool,
        persistent: bool,
    ) -> u16 {
        let offset = self.locals.total_size();
        let size = ty.size();
        let idx = self.locals.slots.len() as u16;
        self.locals.slots.push(VarSlot {
            name: name.to_string(),
            ty,
            offset,
            size,
            retain,
            persistent,
            int_width,
        });
        idx
    }

    fn find_local(&self, name: &str) -> Option<u16> {
        self.locals.find_slot(name).map(|(i, _)| i)
    }

    fn find_global(&self, name: &str) -> Option<u16> {
        self.globals.find_slot(name).map(|(i, _)| i)
    }

    /// Resolve a field index for a user-defined type (FB, class, or struct).
    /// Returns the slot index (position in the FB's `Vec<Value>`).
    fn resolve_field_index(&self, type_name: &str, field_name: &str) -> Option<u16> {
        // Try FB/class: look up function with matching name, then find field in locals
        if let Some(idx) = self.module_functions.iter()
            .find(|f| f.name.eq_ignore_ascii_case(type_name))
            .and_then(|f| f.locals.find_slot(field_name))
            .map(|(i, _)| i)
        {
            return Some(idx);
        }
        // Try struct: look up TypeDef::Struct with matching name, then find field
        for td in self.type_defs.iter() {
            if let TypeDef::Struct { name, fields } = td {
                if name.eq_ignore_ascii_case(type_name) {
                    return fields.iter()
                        .enumerate()
                        .find(|(_, s)| s.name.eq_ignore_ascii_case(field_name))
                        .map(|(i, _)| i as u16);
                }
            }
        }
        None
    }

    /// Resolve a field's expanded offset for native FB array field access.
    /// For native FBs, the offset accounts for inline array expansion.
    fn resolve_field_expanded_offset(&self, type_name: &str, field_name: &str) -> Option<u16> {
        self.module_functions.iter()
            .find(|f| f.name.eq_ignore_ascii_case(type_name))
            .and_then(|f| f.locals.find_slot(field_name))
            .map(|(_, slot)| slot.offset as u16)
    }

    /// Walk the class hierarchy to find a method. Returns the function index.
    fn find_method_in_hierarchy(&self, class_name: &str, method_name: &str) -> Option<usize> {
        // Try ClassName.MethodName at each level of the hierarchy
        let mut current_class = Some(class_name.to_string());
        while let Some(ref cls) = current_class {
            let full_name = format!("{cls}.{method_name}");
            if let Some((idx, _)) = self.module_functions
                .iter()
                .enumerate()
                .find(|(_, f)| f.name.eq_ignore_ascii_case(&full_name))
            {
                return Some(idx);
            }
            // Find the base class by looking at the class function's name pattern
            // The class itself is registered; we need to find EXTENDS info.
            // Since we don't have AST here, check if there's a class function
            // with this name and look for parent pattern in other functions.
            // Walk compiled functions looking for parent class patterns.
            current_class = self.find_base_class(cls);
        }
        None
    }

    /// Find the base class name from the inheritance mapping.
    fn find_base_class(&self, class_name: &str) -> Option<String> {
        self.class_bases.get(&class_name.to_uppercase()).cloned()
    }

    fn finish(self, name: String, kind: PouKind, body_start_pc: usize) -> Function {
        Function {
            name,
            kind,
            register_count: self.next_reg,
            instructions: self.instructions,
            label_positions: self.label_positions,
            locals: self.locals,
            source_map: self.source_map,
            body_start_pc,
        }
    }

    // =========================================================================
    // Variable blocks
    // =========================================================================

    fn compile_var_blocks(&mut self, var_blocks: &[VarBlock]) {
        self.compile_var_blocks_inner(var_blocks, true);
    }

    /// Register var blocks as local slots. If `emit_init` is false, only
    /// create the slots without emitting initializer code (used for class
    /// vars in method bodies — init happens once at class instantiation).
    fn register_var_blocks(&mut self, var_blocks: &[VarBlock]) {
        self.compile_var_blocks_inner(var_blocks, false);
    }

    fn compile_var_blocks_inner(&mut self, var_blocks: &[VarBlock], emit_init: bool) {
        for vb in var_blocks {
            let is_retain = vb.qualifiers.contains(&VarQualifier::Retain);
            let is_persistent = vb.qualifiers.contains(&VarQualifier::Persistent);
            for decl in &vb.declarations {
                let ty = ModuleCompiler::var_type_from_ast(&decl.ty);
                let int_width = ModuleCompiler::int_width_from_ast(&decl.ty);
                // Track FB type names for user-defined types
                let fb_type_name = match &decl.ty {
                    DataType::UserDefined(qn) => Some(qn.as_str()),
                    _ => None,
                };
                for name in &decl.names {
                    let slot = self.add_local_with_qualifiers(
                        &name.name, ty, int_width, is_retain, is_persistent,
                    );
                    // Remember the type name so we can resolve field access later
                    if let Some(ref type_name) = fb_type_name {
                        self.fb_type_names.insert(slot, type_name.clone());
                        // Fix the VarType from the generic Int placeholder to
                        // the actual FB/class/struct type. This is critical for
                        // the debugger's variable display — without it, instance
                        // fields can't be expanded.
                        if let Some(func_idx) = self.module_functions.iter().position(|f| {
                            f.name.eq_ignore_ascii_case(type_name)
                        }) {
                            let kind = self.module_functions[func_idx].kind;
                            if kind == st_ir::PouKind::FunctionBlock {
                                self.locals.slots[slot as usize].ty =
                                    VarType::FbInstance(func_idx as u16);
                            } else if kind == st_ir::PouKind::Class {
                                self.locals.slots[slot as usize].ty =
                                    VarType::ClassInstance(func_idx as u16);
                            }
                        } else if let Some(td_idx) = self.type_defs.iter().position(|td| {
                            matches!(td, TypeDef::Struct { name, .. } if name.eq_ignore_ascii_case(type_name))
                        }) {
                            self.locals.slots[slot as usize].ty =
                                VarType::Struct(td_idx as u16);
                        }
                    }
                    // Resolve array types: create TypeDef::Array and fix VarType
                    if let DataType::Array(arr) = &decl.ty {
                        let elem_ty = ModuleCompiler::var_type_from_ast(&arr.element_type);
                        let dimensions: Vec<(i64, i64)> = arr.ranges.iter().map(|r| {
                            let lo = Self::const_eval_int_expr(&r.lower).unwrap_or(0);
                            let hi = Self::const_eval_int_expr(&r.upper).unwrap_or(0);
                            (lo, hi)
                        }).collect();
                        let td_idx = self.type_defs.len() as u16;
                        self.type_defs.push(TypeDef::Array {
                            element_type: elem_ty,
                            dimensions,
                        });
                        self.locals.slots[slot as usize].ty = VarType::Array(td_idx);
                    }

                    // Emit initializer if present
                    if emit_init {
                        if let Some(init_expr) = &decl.initial_value {
                            let reg = self.compile_expression(init_expr);
                            self.emit(Instruction::StoreLocal(slot, reg));
                        }
                    }
                }
            }
        }
    }

    // =========================================================================
    // Statements
    // =========================================================================

    fn compile_statements(&mut self, stmts: &[Statement]) -> Result<(), CompileError> {
        for stmt in stmts {
            self.compile_statement(stmt)?;
        }
        Ok(())
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), CompileError> {
        // Set source range so the first instruction of this statement
        // gets mapped in the source map (for breakpoints + debugging)
        self.set_source(stmt.range());

        match stmt {
            Statement::Assignment(a) => {
                let val = self.compile_expression(&a.value);
                self.compile_store(&a.target, val, a.range);
            }
            Statement::FunctionCall(fc) => {
                self.compile_function_call(fc);
            }
            Statement::If(if_stmt) => {
                self.compile_if(if_stmt)?;
            }
            Statement::Case(case_stmt) => {
                self.compile_case(case_stmt)?;
            }
            Statement::For(for_stmt) => {
                self.compile_for(for_stmt)?;
            }
            Statement::While(w) => {
                self.compile_while(w)?;
            }
            Statement::Repeat(r) => {
                self.compile_repeat(r)?;
            }
            Statement::Return(range) => {
                let zero = self.alloc_reg();
                self.emit_sourced(Instruction::LoadConst(zero, Value::Int(0)), *range);
                self.emit(Instruction::Ret(zero));
            }
            Statement::Exit(_) => {
                if let Some(&label) = self.loop_exit_labels.last() {
                    self.emit(Instruction::Jump(label));
                }
            }
            Statement::Empty(_) => {}
        }
        Ok(())
    }

    fn compile_store(&mut self, target: &VariableAccess, val_reg: Reg, range: TextRange) {
        // Check for pointer dereference store: ptr^ := value
        if target.parts.len() >= 2 {
            if let (Some(AccessPart::Identifier(id)), Some(AccessPart::Deref)) =
                (target.parts.first(), target.parts.get(1))
            {
                let ptr_reg = self.compile_load_variable(&id.name);
                self.emit_sourced(Instruction::DerefStore(ptr_reg, val_reg), range);
                return;
            }
        }

        // Check for array store: arr[expr] := value
        if target.parts.len() >= 2 {
            if let (Some(AccessPart::Identifier(id)), Some(AccessPart::Index(indices))) =
                (target.parts.first(), target.parts.get(1))
            {
                if let Some(slot) = self.find_local(&id.name) {
                    if !indices.is_empty() {
                        let idx_reg = self.compile_expression(&indices[0]);
                        self.emit_sourced(
                            Instruction::StoreArray(slot, idx_reg, val_reg),
                            range,
                        );
                        return;
                    }
                }
            }
        }

        // Check for field store: fb_instance.field := value or fb_instance.field[i] := value
        if target.parts.len() >= 2 {
            if let (Some(AccessPart::Identifier(obj)), Some(AccessPart::Identifier(field))) =
                (target.parts.first(), target.parts.get(1))
            {
                if let Some(slot) = self.find_local(&obj.name) {
                    if let Some(type_name) = self.fb_type_names.get(&slot).cloned() {
                        let field_idx = self.resolve_field_index(&type_name, &field.name)
                            .unwrap_or_else(|| {
                                eprintln!(
                                    "[COMPILER] warning: field '{}' not found in type '{}'",
                                    field.name, type_name
                                );
                                0
                            });
                        // Check for array indexing: fb.field[expr] := value
                        if let Some(AccessPart::Index(indices)) = target.parts.get(2) {
                            if !indices.is_empty() {
                                let base_offset = self.resolve_field_expanded_offset(&type_name, &field.name)
                                    .unwrap_or(field_idx);
                                let idx_reg = self.compile_expression(&indices[0]);
                                self.emit_sourced(
                                    Instruction::StoreFieldIndex(slot, base_offset, idx_reg, val_reg),
                                    range,
                                );
                                return;
                            }
                        }
                        self.emit_sourced(
                            Instruction::StoreField(slot, field_idx, val_reg),
                            range,
                        );
                        return;
                    }
                }
            }
        }

        // Check for partial store: var.%X0 := value, var.%B1 := value
        if target.parts.len() >= 2 {
            if let (Some(AccessPart::Identifier(id)), Some(AccessPart::Partial(kind, index))) =
                (target.parts.first(), target.parts.get(1))
            {
                let base = self.compile_load_variable(&id.name);
                let dst = self.alloc_reg();
                match kind {
                    PartialAccessKind::Bit => {
                        self.emit(Instruction::InsertBit(dst, base, *index as u8, val_reg));
                    }
                    PartialAccessKind::Byte => {
                        self.emit(Instruction::InsertPartial(dst, base, *index as u8, 8, val_reg));
                    }
                    PartialAccessKind::Word => {
                        self.emit(Instruction::InsertPartial(dst, base, *index as u8, 16, val_reg));
                    }
                    PartialAccessKind::DWord => {
                        self.emit(Instruction::InsertPartial(dst, base, *index as u8, 32, val_reg));
                    }
                    PartialAccessKind::LWord => {
                        self.emit(Instruction::InsertPartial(dst, base, *index as u8, 64, val_reg));
                    }
                }
                // Store modified value back
                if let Some(slot) = self.find_local(&id.name) {
                    self.emit_sourced(Instruction::StoreLocal(slot, dst), range);
                } else if let Some(slot) = self.find_global(&id.name) {
                    self.emit_sourced(Instruction::StoreGlobal(slot, dst), range);
                }
                return;
            }
        }

        if let Some(AccessPart::Identifier(id)) = target.parts.first() {
            if let Some(slot) = self.find_local(&id.name) {
                self.emit_sourced(Instruction::StoreLocal(slot, val_reg), range);
            } else if let Some(slot) = self.find_global(&id.name) {
                self.emit_sourced(Instruction::StoreGlobal(slot, val_reg), range);
            }
        }
    }

    fn compile_if(&mut self, if_stmt: &IfStmt) -> Result<(), CompileError> {
        let end_label = self.alloc_label();
        let else_label = self.alloc_label();

        let cond = self.compile_expression(&if_stmt.condition);
        self.emit(Instruction::JumpIfNot(cond, else_label));
        self.compile_statements(&if_stmt.then_body)?;
        self.emit(Instruction::Jump(end_label));
        self.place_label(else_label);

        for elsif in &if_stmt.elsif_clauses {
            let next_label = self.alloc_label();
            let cond = self.compile_expression(&elsif.condition);
            self.emit(Instruction::JumpIfNot(cond, next_label));
            self.compile_statements(&elsif.body)?;
            self.emit(Instruction::Jump(end_label));
            self.place_label(next_label);
        }

        if let Some(else_body) = &if_stmt.else_body {
            self.compile_statements(else_body)?;
        }

        self.place_label(end_label);
        Ok(())
    }

    fn compile_case(&mut self, case_stmt: &CaseStmt) -> Result<(), CompileError> {
        let end_label = self.alloc_label();
        let expr_reg = self.compile_expression(&case_stmt.expression);

        for branch in &case_stmt.branches {
            let body_label = self.alloc_label();
            let next_label = self.alloc_label();

            // Check each selector
            for selector in &branch.selectors {
                match selector {
                    CaseSelector::Single(val) => {
                        let val_reg = self.compile_expression(val);
                        let cmp = self.alloc_reg();
                        self.emit(Instruction::CmpEq(cmp, expr_reg, val_reg));
                        self.emit(Instruction::JumpIf(cmp, body_label));
                    }
                    CaseSelector::Range(lo, hi) => {
                        let lo_reg = self.compile_expression(lo);
                        let hi_reg = self.compile_expression(hi);
                        let cmp_lo = self.alloc_reg();
                        let cmp_hi = self.alloc_reg();
                        let both = self.alloc_reg();
                        self.emit(Instruction::CmpGe(cmp_lo, expr_reg, lo_reg));
                        self.emit(Instruction::CmpLe(cmp_hi, expr_reg, hi_reg));
                        self.emit(Instruction::And(both, cmp_lo, cmp_hi));
                        self.emit(Instruction::JumpIf(both, body_label));
                    }
                }
            }
            self.emit(Instruction::Jump(next_label));

            self.place_label(body_label);
            self.compile_statements(&branch.body)?;
            self.emit(Instruction::Jump(end_label));
            self.place_label(next_label);
        }

        if let Some(else_body) = &case_stmt.else_body {
            self.compile_statements(else_body)?;
        }

        self.place_label(end_label);
        Ok(())
    }

    fn compile_for(&mut self, for_stmt: &ForStmt) -> Result<(), CompileError> {
        let loop_label = self.alloc_label();
        let exit_label = self.alloc_label();

        // Initialize loop variable
        let from_reg = self.compile_expression(&for_stmt.from);
        if let Some(slot) = self.find_local(&for_stmt.variable.name) {
            self.emit(Instruction::StoreLocal(slot, from_reg));
        }

        self.place_label(loop_label);

        // Check condition: variable <= to
        let var_reg = self.compile_load_variable(&for_stmt.variable.name);
        let to_reg = self.compile_expression(&for_stmt.to);
        let cond = self.alloc_reg();
        self.emit(Instruction::CmpLe(cond, var_reg, to_reg));
        self.emit(Instruction::JumpIfNot(cond, exit_label));

        // Body
        self.loop_exit_labels.push(exit_label);
        self.compile_statements(&for_stmt.body)?;
        self.loop_exit_labels.pop();

        // Increment
        let step_reg = if let Some(by_expr) = &for_stmt.by {
            self.compile_expression(by_expr)
        } else {
            let r = self.alloc_reg();
            self.emit(Instruction::LoadConst(r, Value::Int(1)));
            r
        };
        let var_reg2 = self.compile_load_variable(&for_stmt.variable.name);
        let new_val = self.alloc_reg();
        self.emit(Instruction::Add(new_val, var_reg2, step_reg));
        if let Some(slot) = self.find_local(&for_stmt.variable.name) {
            self.emit(Instruction::StoreLocal(slot, new_val));
        }

        self.emit(Instruction::Jump(loop_label));
        self.place_label(exit_label);
        Ok(())
    }

    fn compile_while(&mut self, w: &WhileStmt) -> Result<(), CompileError> {
        let loop_label = self.alloc_label();
        let exit_label = self.alloc_label();

        self.place_label(loop_label);
        let cond = self.compile_expression(&w.condition);
        self.emit(Instruction::JumpIfNot(cond, exit_label));

        self.loop_exit_labels.push(exit_label);
        self.compile_statements(&w.body)?;
        self.loop_exit_labels.pop();

        self.emit(Instruction::Jump(loop_label));
        self.place_label(exit_label);
        Ok(())
    }

    fn compile_repeat(&mut self, r: &RepeatStmt) -> Result<(), CompileError> {
        let loop_label = self.alloc_label();
        let exit_label = self.alloc_label();

        self.place_label(loop_label);

        self.loop_exit_labels.push(exit_label);
        self.compile_statements(&r.body)?;
        self.loop_exit_labels.pop();

        let cond = self.compile_expression(&r.condition);
        self.emit(Instruction::JumpIfNot(cond, loop_label));
        self.place_label(exit_label);
        Ok(())
    }

    // =========================================================================
    // Expressions
    // =========================================================================

    fn compile_expression(&mut self, expr: &Expression) -> Reg {
        match expr {
            Expression::Literal(lit) => {
                let val = self.literal_to_value(lit);
                let reg = self.alloc_reg();
                self.emit(Instruction::LoadConst(reg, val));
                reg
            }
            Expression::Variable(va) => {
                if va.parts.len() >= 2 {
                    // Check for pointer dereference: ptr^
                    if let (Some(AccessPart::Identifier(id)), Some(AccessPart::Deref)) =
                        (va.parts.first(), va.parts.get(1))
                    {
                        let ptr_reg = self.compile_load_variable(&id.name);
                        let dst = self.alloc_reg();
                        self.emit(Instruction::Deref(dst, ptr_reg));
                        return dst;
                    }
                    // Array indexing: arr[expr]
                    if let (Some(AccessPart::Identifier(id)), Some(AccessPart::Index(indices))) =
                        (va.parts.first(), va.parts.get(1))
                    {
                        if let Some(slot) = self.find_local(&id.name) {
                            if !indices.is_empty() {
                                let idx_reg = self.compile_expression(&indices[0]);
                                let dst = self.alloc_reg();
                                self.emit(Instruction::LoadArray(dst, slot, idx_reg));
                                return dst;
                            }
                        }
                    }
                    // Multi-part access: fb_instance.field or fb_instance.field[index]
                    if let (Some(AccessPart::Identifier(obj)), Some(AccessPart::Identifier(field))) =
                        (va.parts.first(), va.parts.get(1))
                    {
                        if let Some(slot) = self.find_local(&obj.name) {
                            if let Some(type_name) = self.fb_type_names.get(&slot).cloned() {
                                let field_idx = self.resolve_field_index(&type_name, &field.name)
                                    .unwrap_or_else(|| {
                                        eprintln!(
                                            "[COMPILER] warning: field '{}' not found in type '{}' — \
                                             was the type compiled before its caller?",
                                            field.name, type_name
                                        );
                                        0
                                    });
                                // Check for array indexing: fb.field[expr]
                                if let Some(AccessPart::Index(indices)) = va.parts.get(2) {
                                    if !indices.is_empty() {
                                        // Use expanded offset for array field access
                                        let base_offset = self.resolve_field_expanded_offset(&type_name, &field.name)
                                            .unwrap_or(field_idx);
                                        let idx_reg = self.compile_expression(&indices[0]);
                                        let dst = self.alloc_reg();
                                        self.emit(Instruction::LoadFieldIndex(dst, slot, base_offset, idx_reg));
                                        return dst;
                                    }
                                }
                                let dst = self.alloc_reg();
                                self.emit(Instruction::LoadField(dst, slot, field_idx));
                                return dst;
                            }
                        }
                    }
                }
                // Check for partial access on a simple variable: var.%X0, var.%B1, etc.
                if va.parts.len() >= 2 {
                    if let Some(AccessPart::Identifier(id)) = va.parts.first() {
                        if let Some(AccessPart::Partial(kind, index)) = va.parts.get(1) {
                            let base = self.compile_load_variable(&id.name);
                            return self.compile_partial_read(base, *kind, *index);
                        }
                    }
                }
                if let Some(AccessPart::Identifier(id)) = va.parts.first() {
                    let base = self.compile_load_variable(&id.name);
                    // Check for chained partial access after the base
                    self.apply_partial_chain(base, &va.parts[1..])
                } else {
                    let reg = self.alloc_reg();
                    self.emit(Instruction::LoadConst(reg, Value::Int(0)));
                    reg
                }
            }
            Expression::Binary(b) => {
                let left = self.compile_expression(&b.left);
                let right = self.compile_expression(&b.right);
                let dst = self.alloc_reg();
                let instr = match b.op {
                    BinaryOp::Add => Instruction::Add(dst, left, right),
                    BinaryOp::Sub => Instruction::Sub(dst, left, right),
                    BinaryOp::Mul => Instruction::Mul(dst, left, right),
                    BinaryOp::Div => Instruction::Div(dst, left, right),
                    BinaryOp::Mod => Instruction::Mod(dst, left, right),
                    BinaryOp::Power => Instruction::Pow(dst, left, right),
                    BinaryOp::And => Instruction::And(dst, left, right),
                    BinaryOp::Or => Instruction::Or(dst, left, right),
                    BinaryOp::Xor => Instruction::Xor(dst, left, right),
                    BinaryOp::Eq => Instruction::CmpEq(dst, left, right),
                    BinaryOp::Ne => Instruction::CmpNe(dst, left, right),
                    BinaryOp::Lt => Instruction::CmpLt(dst, left, right),
                    BinaryOp::Gt => Instruction::CmpGt(dst, left, right),
                    BinaryOp::Le => Instruction::CmpLe(dst, left, right),
                    BinaryOp::Ge => Instruction::CmpGe(dst, left, right),
                };
                self.emit(instr);
                dst
            }
            Expression::Unary(u) => {
                let operand = self.compile_expression(&u.operand);
                let dst = self.alloc_reg();
                match u.op {
                    UnaryOp::Neg => self.emit(Instruction::Neg(dst, operand)),
                    UnaryOp::Not => self.emit(Instruction::Not(dst, operand)),
                };
                dst
            }
            Expression::FunctionCall(fc) => {
                self.compile_function_call_expr(fc)
            }
            Expression::Parenthesized(inner) => self.compile_expression(inner),
            Expression::This(_) | Expression::Super(_) => {
                // THIS/SUPER compile to loading the instance slot (slot 0 by convention)
                let reg = self.alloc_reg();
                self.emit(Instruction::LoadConst(reg, Value::Int(0)));
                reg
            }
        }
    }

    /// Emit extraction for a partial access (bit, byte, word, dword).
    fn compile_partial_read(&mut self, src: Reg, kind: PartialAccessKind, index: u32) -> Reg {
        let dst = self.alloc_reg();
        match kind {
            PartialAccessKind::Bit => {
                self.emit(Instruction::ExtractBit(dst, src, index as u8));
            }
            PartialAccessKind::Byte => {
                self.emit(Instruction::ExtractPartial(dst, src, index as u8, 8));
            }
            PartialAccessKind::Word => {
                self.emit(Instruction::ExtractPartial(dst, src, index as u8, 16));
            }
            PartialAccessKind::DWord => {
                self.emit(Instruction::ExtractPartial(dst, src, index as u8, 32));
            }
            PartialAccessKind::LWord => {
                self.emit(Instruction::ExtractPartial(dst, src, index as u8, 64));
            }
        }
        dst
    }

    /// Apply any partial access parts after the base, returning the final register.
    fn apply_partial_chain(&mut self, mut reg: Reg, parts: &[AccessPart]) -> Reg {
        for part in parts {
            if let AccessPart::Partial(kind, index) = part {
                reg = self.compile_partial_read(reg, *kind, *index);
            }
        }
        reg
    }

    fn compile_load_variable(&mut self, name: &str) -> Reg {
        let reg = self.alloc_reg();
        if let Some(slot) = self.find_local(name) {
            self.emit(Instruction::LoadLocal(reg, slot));
        } else if let Some(slot) = self.find_global(name) {
            self.emit(Instruction::LoadGlobal(reg, slot));
        } else {
            self.emit(Instruction::LoadConst(reg, Value::Int(0)));
        }
        reg
    }

    fn compile_function_call(&mut self, fc: &FunctionCallExpr) -> Reg {
        self.compile_function_call_expr(fc)
    }

    fn compile_function_call_expr(&mut self, fc: &FunctionCallExpr) -> Reg {
        let name = fc.name.as_str();
        let dst = self.alloc_reg();

        // Handle method calls: instance.Method(args)
        if fc.name.parts.len() == 2 {
            let obj_name = &fc.name.parts[0].name;
            let method_name = &fc.name.parts[1].name;
            if let Some(slot) = self.find_local(obj_name) {
                if let Some(type_name) = self.fb_type_names.get(&slot).cloned() {
                    // Find the class function index (for instance state management)
                    let class_func_idx = self.module_functions
                        .iter()
                        .position(|f| f.name.eq_ignore_ascii_case(&type_name))
                        .unwrap_or(0) as u16;
                    // Walk the class hierarchy to find the method
                    let func_idx = self.find_method_in_hierarchy(&type_name, method_name);
                    if let Some(idx) = func_idx {
                        let args = self.compile_call_args(&fc.arguments);
                        self.emit(Instruction::CallMethod {
                            instance_slot: slot,
                            class_func_index: class_func_idx,
                            func_index: idx as u16,
                            dst,
                            args,
                        });
                        return dst;
                    }
                }
            }
        }

        // REF() intrinsic — takes a variable name and returns a reference
        if name.to_uppercase() == "REF" {
            if let Some(first_arg) = fc.arguments.first() {
                let var_name = match first_arg {
                    Argument::Positional(Expression::Variable(va)) => {
                        va.parts.first().and_then(|p| match p {
                            AccessPart::Identifier(id) => Some(id.name.clone()),
                            _ => None,
                        })
                    }
                    Argument::Named { value: Expression::Variable(va), .. } => {
                        va.parts.first().and_then(|p| match p {
                            AccessPart::Identifier(id) => Some(id.name.clone()),
                            _ => None,
                        })
                    }
                    _ => None,
                };
                if let Some(name) = var_name {
                    if let Some(slot) = self.find_local(&name) {
                        self.emit(Instruction::MakeRefLocal(dst, slot));
                    } else if let Some(slot) = self.find_global(&name) {
                        self.emit(Instruction::MakeRefGlobal(dst, slot));
                    } else {
                        self.emit(Instruction::LoadNull(dst));
                    }
                } else {
                    self.emit(Instruction::LoadNull(dst));
                }
            } else {
                self.emit(Instruction::LoadNull(dst));
            }
            return dst;
        }

        // Check for zero-argument intrinsics
        if name.to_uppercase() == "SYSTEM_TIME" {
            self.emit(Instruction::SystemTime(dst));
            return dst;
        }

        // Check for single-argument intrinsics (math + type conversions)
        let intrinsic: Option<fn(Reg, Reg) -> Instruction> = match name.to_uppercase().as_str() {
            // Math
            "SQRT" => Some(Instruction::Sqrt),
            "SIN"  => Some(Instruction::Sin),
            "COS"  => Some(Instruction::Cos),
            "TAN"  => Some(Instruction::Tan),
            "ASIN" => Some(Instruction::Asin),
            "ACOS" => Some(Instruction::Acos),
            "ATAN" => Some(Instruction::Atan),
            "LN"   => Some(Instruction::Ln),
            "LOG"  => Some(Instruction::Log),
            "EXP"  => Some(Instruction::Exp),
            // Type conversions: *_TO_REAL
            "INT_TO_REAL" | "SINT_TO_REAL" | "DINT_TO_REAL" | "LINT_TO_REAL"
            | "UINT_TO_REAL" | "UDINT_TO_REAL" | "ULINT_TO_REAL" | "USINT_TO_REAL"
            | "BOOL_TO_REAL" | "INT_TO_LREAL" | "SINT_TO_LREAL" | "DINT_TO_LREAL"
            | "LINT_TO_LREAL" | "REAL_TO_LREAL"
            | "TIME_TO_REAL" | "TIME_TO_LREAL"
            | "TO_REAL" | "TO_LREAL" | "ANY_TO_REAL" | "ANY_TO_LREAL"
                => Some(Instruction::ToReal),
            // *_TO_INT
            "REAL_TO_INT" | "LREAL_TO_INT" | "REAL_TO_DINT" | "LREAL_TO_DINT"
            | "REAL_TO_LINT" | "LREAL_TO_LINT" | "REAL_TO_SINT" | "LREAL_TO_SINT"
            | "BOOL_TO_INT" | "BOOL_TO_DINT" | "BOOL_TO_LINT"
            | "UINT_TO_INT" | "UDINT_TO_DINT" | "ULINT_TO_LINT"
            | "INT_TO_DINT" | "INT_TO_LINT" | "DINT_TO_LINT"
            | "SINT_TO_INT" | "SINT_TO_DINT" | "SINT_TO_LINT"
            | "TIME_TO_INT" | "TIME_TO_SINT" | "TIME_TO_DINT" | "TIME_TO_LINT"
            | "TIME_TO_UINT" | "TIME_TO_USINT" | "TIME_TO_UDINT" | "TIME_TO_ULINT"
            | "TO_INT" | "TO_SINT" | "TO_DINT" | "TO_LINT"
            | "TO_UINT" | "TO_USINT" | "TO_UDINT" | "TO_ULINT"
            | "ANY_TO_INT" | "ANY_TO_SINT" | "ANY_TO_DINT" | "ANY_TO_LINT"
            | "ANY_TO_UINT" | "ANY_TO_USINT" | "ANY_TO_UDINT" | "ANY_TO_ULINT"
                => Some(Instruction::ToInt),
            // *_TO_BOOL
            "INT_TO_BOOL" | "REAL_TO_BOOL" | "DINT_TO_BOOL" | "LINT_TO_BOOL"
            | "TIME_TO_BOOL"
            | "TO_BOOL" | "ANY_TO_BOOL"
                => Some(Instruction::ToBool),
            // *_TO_TIME / *_TO_DATE / *_TO_TOD / *_TO_DT
            // All date/time types share Value::Time(i64) in milliseconds.
            // Conversions between them and from numerics all use ToTime.
            "INT_TO_TIME" | "SINT_TO_TIME" | "DINT_TO_TIME" | "LINT_TO_TIME"
            | "UINT_TO_TIME" | "USINT_TO_TIME" | "UDINT_TO_TIME" | "ULINT_TO_TIME"
            | "REAL_TO_TIME" | "LREAL_TO_TIME" | "BOOL_TO_TIME"
            | "TO_TIME" | "ANY_TO_TIME"
            | "INT_TO_DATE" | "SINT_TO_DATE" | "DINT_TO_DATE" | "LINT_TO_DATE"
            | "UINT_TO_DATE" | "USINT_TO_DATE" | "UDINT_TO_DATE" | "ULINT_TO_DATE"
            | "REAL_TO_DATE" | "LREAL_TO_DATE"
            | "TO_DATE" | "ANY_TO_DATE"
                => Some(Instruction::ToTime),
            // *_TO_TOD — wraps modulo 86_400_000 (24 hours)
            "INT_TO_TOD" | "SINT_TO_TOD" | "DINT_TO_TOD" | "LINT_TO_TOD"
            | "UINT_TO_TOD" | "USINT_TO_TOD" | "UDINT_TO_TOD" | "ULINT_TO_TOD"
            | "REAL_TO_TOD" | "LREAL_TO_TOD"
            | "TO_TOD" | "ANY_TO_TOD"
            | "TIME_TO_TOD"
                => Some(Instruction::ToTod),
            // *_TO_DT / cross-type casts (no wrapping needed)
            "INT_TO_DT" | "SINT_TO_DT" | "DINT_TO_DT" | "LINT_TO_DT"
            | "UINT_TO_DT" | "USINT_TO_DT" | "UDINT_TO_DT" | "ULINT_TO_DT"
            | "REAL_TO_DT" | "LREAL_TO_DT"
            | "TO_DT" | "ANY_TO_DT"
            // Cross-type identity casts (all just pass through the ms value)
            | "DATE_TO_DT" | "TIME_TO_DATE" | "TIME_TO_DT"
            | "DATE_TO_TIME" | "TOD_TO_TIME" | "DT_TO_TIME"
                => Some(Instruction::ToTime),
            // DATE/TOD/DT _TO_INT (same as TIME — returns raw ms)
            "DATE_TO_INT" | "DATE_TO_SINT" | "DATE_TO_DINT" | "DATE_TO_LINT"
            | "DATE_TO_UINT" | "DATE_TO_USINT" | "DATE_TO_UDINT" | "DATE_TO_ULINT"
            | "TOD_TO_INT" | "TOD_TO_SINT" | "TOD_TO_DINT" | "TOD_TO_LINT"
            | "TOD_TO_UINT" | "TOD_TO_USINT" | "TOD_TO_UDINT" | "TOD_TO_ULINT"
            | "DT_TO_INT" | "DT_TO_SINT" | "DT_TO_DINT" | "DT_TO_LINT"
            | "DT_TO_UINT" | "DT_TO_USINT" | "DT_TO_UDINT" | "DT_TO_ULINT"
                => Some(Instruction::ToInt),
            "DATE_TO_REAL" | "DATE_TO_LREAL"
            | "TOD_TO_REAL" | "TOD_TO_LREAL"
            | "DT_TO_REAL" | "DT_TO_LREAL"
                => Some(Instruction::ToReal),
            "DATE_TO_BOOL" | "TOD_TO_BOOL" | "DT_TO_BOOL"
                => Some(Instruction::ToBool),
            // DT extraction
            "DT_TO_DATE" => Some(Instruction::DtExtractDate),
            "DT_TO_TOD" => Some(Instruction::DtExtractTod),
            "DAY_OF_WEEK" => Some(Instruction::DayOfWeek),
            // String — single-arg
            "LEN" => Some(Instruction::StringLen),
            "TRIM" => Some(Instruction::StringTrim),
            "LTRIM" => Some(Instruction::StringLTrim),
            "RTRIM" => Some(Instruction::StringRTrim),
            "TO_UPPER" | "UPPER_CASE" => Some(Instruction::StringToUpper),
            "TO_LOWER" | "LOWER_CASE" => Some(Instruction::StringToLower),
            // *_TO_STRING (signed integer width — VM reads i64)
            "INT_TO_STRING" | "SINT_TO_STRING" | "DINT_TO_STRING" | "LINT_TO_STRING"
                => Some(Instruction::IntToString),
            // *_TO_STRING (unsigned integer width — VM reads u64)
            "UINT_TO_STRING" | "USINT_TO_STRING" | "UDINT_TO_STRING" | "ULINT_TO_STRING"
                => Some(Instruction::UIntToString),
            "REAL_TO_STRING" | "LREAL_TO_STRING" => Some(Instruction::RealToString),
            "BOOL_TO_STRING" => Some(Instruction::BoolToString),
            // STRING → numeric / bool
            "STRING_TO_INT" | "STRING_TO_SINT" | "STRING_TO_DINT" | "STRING_TO_LINT"
                => Some(Instruction::StringToInt),
            "STRING_TO_UINT" | "STRING_TO_USINT" | "STRING_TO_UDINT" | "STRING_TO_ULINT"
                => Some(Instruction::StringToUInt),
            "STRING_TO_REAL" | "STRING_TO_LREAL" => Some(Instruction::StringToReal),
            "STRING_TO_BOOL" => Some(Instruction::StringToBool),
            "TO_STRING" | "ANY_TO_STRING" => Some(Instruction::ToString),
            _ => None,
        };
        if let Some(make_instr) = intrinsic {
            let arg = if let Some(first_arg) = fc.arguments.first() {
                match first_arg {
                    Argument::Positional(expr) => self.compile_expression(expr),
                    Argument::Named { value, .. } => self.compile_expression(value),
                }
            } else {
                let r = self.alloc_reg();
                self.emit(Instruction::LoadConst(r, Value::Real(0.0)));
                r
            };
            self.emit(make_instr(dst, arg));
            return dst;
        }

        // Check for two-argument intrinsics (date/time arithmetic)
        // wrap_tod: result must be wrapped to 0..86_399_999 ms (TOD range)
        type BinIntrinsic = (fn(Reg, Reg, Reg) -> Instruction, bool);
        let intrinsic2: Option<BinIntrinsic> = match name.to_uppercase().as_str() {
            "ADD_TOD_TIME" => Some((Instruction::Add, true)),
            "SUB_TOD_TIME" => Some((Instruction::Sub, true)),
            "ADD_DT_TIME" | "CONCAT_DATE_TOD" => Some((Instruction::Add, false)),
            "SUB_DATE_DATE" | "SUB_TOD_TOD"
            | "SUB_DT_TIME" | "SUB_DT_DT" => Some((Instruction::Sub, false)),
            "MULTIME" => Some((Instruction::Mul, false)),
            "DIVTIME" => Some((Instruction::Div, false)),
            _ => None,
        };
        if let Some((make_instr, wrap_tod)) = intrinsic2 {
            let (arg1, arg2) = match fc.arguments.len() {
                0 => {
                    let r1 = self.alloc_reg();
                    let r2 = self.alloc_reg();
                    self.emit(Instruction::LoadConst(r1, Value::Time(0)));
                    self.emit(Instruction::LoadConst(r2, Value::Time(0)));
                    (r1, r2)
                }
                1 => {
                    let a = match &fc.arguments[0] {
                        Argument::Positional(expr) => self.compile_expression(expr),
                        Argument::Named { value, .. } => self.compile_expression(value),
                    };
                    let r2 = self.alloc_reg();
                    self.emit(Instruction::LoadConst(r2, Value::Time(0)));
                    (a, r2)
                }
                _ => {
                    let a = match &fc.arguments[0] {
                        Argument::Positional(expr) => self.compile_expression(expr),
                        Argument::Named { value, .. } => self.compile_expression(value),
                    };
                    let b = match &fc.arguments[1] {
                        Argument::Positional(expr) => self.compile_expression(expr),
                        Argument::Named { value, .. } => self.compile_expression(value),
                    };
                    (a, b)
                }
            };
            if wrap_tod {
                let tmp = self.alloc_reg();
                self.emit(make_instr(tmp, arg1, arg2));
                self.emit(Instruction::ToTod(dst, tmp));
            } else {
                self.emit(make_instr(dst, arg1, arg2));
            }
            return dst;
        }

        // Two-argument string intrinsics: CONCAT, FIND, LEFT, RIGHT.
        let str_intrinsic2: Option<fn(Reg, Reg, Reg) -> Instruction> =
            match name.to_uppercase().as_str() {
                "CONCAT" => Some(Instruction::StringConcat),
                "FIND" => Some(Instruction::StringFind),
                "LEFT" => Some(Instruction::StringLeft),
                "RIGHT" => Some(Instruction::StringRight),
                _ => None,
            };
        if let Some(make_instr) = str_intrinsic2 {
            let arg1 = self.compile_call_arg(fc, 0);
            let arg2 = self.compile_call_arg(fc, 1);
            self.emit(make_instr(dst, arg1, arg2));
            return dst;
        }

        // Three-argument string intrinsics: MID(STR, LEN, POS), INSERT(STR1, STR2, POS),
        // DELETE(STR, LEN, POS). All map to (dst, src1, src2, src3) instructions.
        type TernaryStrFn = fn(Reg, Reg, Reg, Reg) -> Instruction;
        let str_intrinsic3: Option<TernaryStrFn> = match name.to_uppercase().as_str() {
            "MID" => Some(Instruction::StringMid),
            "INSERT" => Some(Instruction::StringInsert),
            "DELETE" => Some(Instruction::StringDelete),
            _ => None,
        };
        if let Some(make_instr) = str_intrinsic3 {
            let a1 = self.compile_call_arg(fc, 0);
            let a2 = self.compile_call_arg(fc, 1);
            let a3 = self.compile_call_arg(fc, 2);
            self.emit(make_instr(dst, a1, a2, a3));
            return dst;
        }

        // Four-argument string intrinsic: REPLACE(STR1, STR2, LEN, POS).
        if name.eq_ignore_ascii_case("REPLACE") {
            let str1 = self.compile_call_arg(fc, 0);
            let str2 = self.compile_call_arg(fc, 1);
            let len = self.compile_call_arg(fc, 2);
            let pos = self.compile_call_arg(fc, 3);
            self.emit(Instruction::StringReplace { dst, str1, str2, len, pos });
            return dst;
        }

        // Check if it's an FB instance call (local variable whose type is a known FB)
        if let Some(slot) = self.find_local(&name) {
            // Look up the FB TYPE name for this local slot
            let fb_type = self.fb_type_names.get(&slot).cloned().unwrap_or(name.clone());
            let func_idx = self.module_functions
                .iter()
                .position(|f| f.name.eq_ignore_ascii_case(&fb_type))
                .unwrap_or(0) as u16;
            let args = self.compile_call_args(&fc.arguments);
            self.emit(Instruction::CallFb {
                instance_slot: slot,
                func_index: func_idx,
                args,
            });
        } else if let Some((func_idx, _)) = self.module_functions
            .iter()
            .enumerate()
            .find(|(_, f)| f.name.eq_ignore_ascii_case(&name))
        {
            let args = self.compile_call_args(&fc.arguments);
            self.emit(Instruction::Call {
                func_index: func_idx as u16,
                dst,
                args,
            });
        } else {
            self.emit(Instruction::LoadConst(dst, Value::Int(0)));
        }
        dst
    }

    fn compile_call_args(&mut self, arguments: &[Argument]) -> Vec<(u16, Reg)> {
        arguments
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                let reg = match arg {
                    Argument::Positional(expr) => self.compile_expression(expr),
                    Argument::Named { value, .. } => self.compile_expression(value),
                };
                (i as u16, reg)
            })
            .collect()
    }

    /// Compile the i-th argument of a function call (positional or named, name ignored).
    /// If the argument is missing, emits a `LoadConst(Int(0))` to a fresh register so
    /// downstream code always has a valid register to read.
    fn compile_call_arg(&mut self, fc: &FunctionCallExpr, index: usize) -> Reg {
        if let Some(arg) = fc.arguments.get(index) {
            match arg {
                Argument::Positional(expr) => self.compile_expression(expr),
                Argument::Named { value, .. } => self.compile_expression(value),
            }
        } else {
            let r = self.alloc_reg();
            self.emit(Instruction::LoadConst(r, Value::Int(0)));
            r
        }
    }

    fn literal_to_value(&self, lit: &Literal) -> Value {
        match &lit.kind {
            LiteralKind::Integer(v) => Value::Int(*v),
            LiteralKind::Real(v) => Value::Real(*v),
            LiteralKind::Bool(v) => Value::Bool(*v),
            LiteralKind::String(s) => Value::String(s.clone()),
            LiteralKind::Time(s) => Value::Time(parse_time_literal(s)),
            LiteralKind::Date(s) => Value::Time(parse_date_literal(s)),
            LiteralKind::Tod(s) => Value::Time(parse_tod_literal(s)),
            LiteralKind::Dt(s) => Value::Time(parse_dt_literal(s)),
            LiteralKind::Null => Value::Null,
            LiteralKind::Typed { raw_value, .. } => {
                if let Ok(v) = raw_value.parse::<i64>() {
                    Value::Int(v)
                } else if let Ok(v) = raw_value.parse::<f64>() {
                    Value::Real(v)
                } else {
                    Value::Int(0)
                }
            }
        }
    }
}

/// Parse a TIME literal string like "T#5s", "T#100ms", "T#1h2m3s" into milliseconds.
fn parse_time_literal(s: &str) -> i64 {
    let raw = s.trim();
    // Strip prefix
    let body = if let Some(rest) = raw.strip_prefix("T#").or_else(|| raw.strip_prefix("t#")) {
        rest
    } else if let Some(rest) = raw.strip_prefix("TIME#").or_else(|| raw.strip_prefix("time#")) {
        rest
    } else if let Some(rest) = raw.strip_prefix("LT#").or_else(|| raw.strip_prefix("lt#")) {
        rest
    } else if let Some(rest) = raw.strip_prefix("LTIME#").or_else(|| raw.strip_prefix("ltime#")) {
        rest
    } else {
        raw
    };

    let mut total_ms: i64 = 0;
    let mut num_buf = String::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_digit() || ch == '.' || ch == '_' {
            if ch != '_' {
                num_buf.push(ch);
            }
            i += 1;
        } else {
            let num: f64 = num_buf.parse().unwrap_or(0.0);
            num_buf.clear();

            // Check for multi-char units: ms, us
            let unit = if i + 1 < chars.len() {
                let two = format!("{}{}", ch, chars[i + 1]).to_lowercase();
                if two == "ms" || two == "us" {
                    i += 2;
                    two
                } else {
                    i += 1;
                    ch.to_lowercase().to_string()
                }
            } else {
                i += 1;
                ch.to_lowercase().to_string()
            };

            match unit.as_str() {
                "d" => total_ms += (num * 86_400_000.0) as i64,
                "h" => total_ms += (num * 3_600_000.0) as i64,
                "m" => total_ms += (num * 60_000.0) as i64,
                "s" => total_ms += (num * 1_000.0) as i64,
                "ms" => total_ms += num as i64,
                "us" => total_ms += (num / 1000.0) as i64,
                _ => {}
            }
        }
    }

    // Trailing number without unit = ms
    if !num_buf.is_empty() {
        let num: f64 = num_buf.parse().unwrap_or(0.0);
        total_ms += num as i64;
    }

    total_ms
}

/// Convert year/month/day to milliseconds since Unix epoch (1970-01-01).
/// Uses a simplified algorithm (no leap-second handling).
fn ymd_to_epoch_ms(year: i64, month: i64, day: i64) -> i64 {
    // Days from epoch using a standard civil-date algorithm.
    // Adjust so March=1 to simplify leap year logic.
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let days = 365 * y + y / 4 - y / 100 + y / 400
        + (m * 306 + 5) / 10
        + (day - 1)
        - 719468; // offset to Unix epoch
    days * 86_400_000
}

/// Parse a DATE literal like "D#2024-01-15" into milliseconds since epoch.
fn parse_date_literal(s: &str) -> i64 {
    let raw = s.trim();
    // Strip prefix: D#, DATE#, LD#, LDATE#
    let body = raw.split('#').next_back().unwrap_or("");
    let parts: Vec<&str> = body.split('-').collect();
    if parts.len() < 3 {
        return 0;
    }
    let year = parts[0].parse::<i64>().unwrap_or(1970);
    let month = parts[1].parse::<i64>().unwrap_or(1);
    let day = parts[2].parse::<i64>().unwrap_or(1);
    ymd_to_epoch_ms(year, month, day)
}

/// Parse a TOD literal like "TOD#12:30:00" or "TOD#12:30:00.500" into ms since midnight.
/// Values are wrapped modulo 86_400_000 (24 hours) to match CODESYS behavior.
fn parse_tod_literal(s: &str) -> i64 {
    let raw = s.trim();
    let body = raw.split('#').next_back().unwrap_or("");
    // Split time and optional fractional seconds
    let (time_part, frac) = if let Some((t, f)) = body.split_once('.') {
        let frac_str = format!("0.{f}");
        (t, frac_str.parse::<f64>().unwrap_or(0.0))
    } else {
        (body, 0.0)
    };
    let parts: Vec<&str> = time_part.split(':').collect();
    let h = parts.first().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let m = parts.get(1).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let sec = parts.get(2).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let total = h * 3_600_000 + m * 60_000 + sec * 1_000 + (frac * 1000.0) as i64;
    total.rem_euclid(86_400_000)
}

/// Parse a DT literal like "DT#2024-01-15-12:30:00" into ms since epoch.
fn parse_dt_literal(s: &str) -> i64 {
    let raw = s.trim();
    let body = raw.split('#').next_back().unwrap_or("");
    // Format: YYYY-M-D-HH:MM:SS[.frac]
    // Split on '-' → [YYYY, M, D, HH:MM:SS.frac]
    let parts: Vec<&str> = body.splitn(4, '-').collect();
    if parts.len() < 4 {
        return 0;
    }
    let year = parts[0].parse::<i64>().unwrap_or(1970);
    let month = parts[1].parse::<i64>().unwrap_or(1);
    let day = parts[2].parse::<i64>().unwrap_or(1);
    let date_ms = ymd_to_epoch_ms(year, month, day);
    // Parse the time portion by prefixing with "TOD#"
    let tod_ms = parse_tod_literal(&format!("TOD#{}", parts[3]));
    date_ms + tod_ms
}

