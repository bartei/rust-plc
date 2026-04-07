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
    let mut ctx = ModuleCompiler {
        functions: Vec::new(),
        globals: MemoryLayout::default(),
        type_defs: Vec::new(),
        class_bases: std::collections::HashMap::new(),
        class_var_blocks: std::collections::HashMap::new(),
    };
    // Pass 1: register all POUs so cross-references work
    for item in &source_file.items {
        ctx.register_item(item);
    }
    // Pass 2: compile bodies
    for item in &source_file.items {
        ctx.compile_item(item)?;
    }
    Ok(Module {
        functions: ctx.functions,
        globals: ctx.globals,
        type_defs: ctx.type_defs,
    })
}

struct ModuleCompiler {
    functions: Vec<Function>,
    globals: MemoryLayout,
    type_defs: Vec<TypeDef>,
    /// Maps class name → base class name (for inheritance chain resolution).
    class_bases: std::collections::HashMap<String, String>,
    /// Maps class name → var_blocks (for inherited var access in methods).
    class_var_blocks: std::collections::HashMap<String, Vec<VarBlock>>,
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
                    for name in &decl.names {
                        let offset = self.globals.total_size();
                        let size = ty.size();
                        self.globals.slots.push(VarSlot {
                            name: name.name.clone(),
                            ty,
                            offset,
                            size,
                            retain: vb.qualifiers.contains(&VarQualifier::Retain),
                        });
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
            TopLevelItem::TypeDeclaration(_) => {
                // Type defs are used at compile time, not registered as functions
            }
        }
    }

    fn compile_item(&mut self, item: &TopLevelItem) -> Result<(), CompileError> {
        match item {
            TopLevelItem::Program(p) => {
                let func_idx = self.find_func(&p.name.name)?;
                let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases);
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
                let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases);
                fc.compile_var_blocks(&f.var_blocks);
                let ret_ty = Self::var_type_from_ast(&f.return_type);
                let ret_slot = fc.add_local(&f.name.name, ret_ty);
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
                let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases);
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
                let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases);
                // Inherited vars (with init, so defaults from parent classes are applied)
                let inherited_for_class = self.collect_inherited_var_blocks(&cls.name.name);
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
                    let mut fc = FunctionCompiler::new(&self.functions, &self.globals, &self.class_bases);
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
                        fc.add_local(&method.name.name, ret_ty)
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
            DataType::Array(_) => VarType::Int, // simplified: arrays handled separately
            DataType::UserDefined(_) => VarType::Int, // simplified: resolved at link time
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

    fn add_local(&mut self, name: &str, ty: VarType) -> u16 {
        let offset = self.locals.total_size();
        let size = ty.size();
        let idx = self.locals.slots.len() as u16;
        self.locals.slots.push(VarSlot {
            name: name.to_string(),
            ty,
            offset,
            size,
            retain: false,
        });
        idx
    }

    fn find_local(&self, name: &str) -> Option<u16> {
        self.locals.find_slot(name).map(|(i, _)| i)
    }

    fn find_global(&self, name: &str) -> Option<u16> {
        self.globals.find_slot(name).map(|(i, _)| i)
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
            for decl in &vb.declarations {
                let ty = ModuleCompiler::var_type_from_ast(&decl.ty);
                // Track FB type names for user-defined types
                let fb_type_name = match &decl.ty {
                    DataType::UserDefined(qn) => Some(qn.as_str()),
                    _ => None,
                };
                for name in &decl.names {
                    let slot = self.add_local(&name.name, ty);
                    // Remember the FB type name so we can resolve calls later
                    if let Some(ref type_name) = fb_type_name {
                        self.fb_type_names.insert(slot, type_name.clone());
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

        // Check for field store: fb_instance.field := value
        if target.parts.len() >= 2 {
            if let (Some(AccessPart::Identifier(obj)), Some(AccessPart::Identifier(field))) =
                (target.parts.first(), target.parts.get(1))
            {
                if let Some(slot) = self.find_local(&obj.name) {
                    if self.fb_type_names.contains_key(&slot) {
                        let fb_type = self.fb_type_names.get(&slot).unwrap().clone();
                        let field_idx = self.module_functions
                            .iter()
                            .find(|f| f.name.eq_ignore_ascii_case(&fb_type))
                            .and_then(|f| f.locals.find_slot(&field.name))
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        self.emit_sourced(
                            Instruction::StoreField(slot, field_idx, val_reg),
                            range,
                        );
                        return;
                    }
                }
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
                    // Multi-part access: fb_instance.field
                    if let (Some(AccessPart::Identifier(obj)), Some(AccessPart::Identifier(field))) =
                        (va.parts.first(), va.parts.get(1))
                    {
                        if let Some(slot) = self.find_local(&obj.name) {
                            if self.fb_type_names.contains_key(&slot) {
                                let fb_type = self.fb_type_names.get(&slot).unwrap().clone();
                                let field_idx = self.module_functions
                                    .iter()
                                    .find(|f| f.name.eq_ignore_ascii_case(&fb_type))
                                    .and_then(|f| f.locals.find_slot(&field.name))
                                    .map(|(i, _)| i)
                                    .unwrap_or(0);
                                let dst = self.alloc_reg();
                                self.emit(Instruction::LoadField(dst, slot, field_idx));
                                return dst;
                            }
                        }
                    }
                }
                if let Some(AccessPart::Identifier(id)) = va.parts.first() {
                    self.compile_load_variable(&id.name)
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
            // Type conversions: *_TO_INT, *_TO_REAL, *_TO_BOOL
            "INT_TO_REAL" | "SINT_TO_REAL" | "DINT_TO_REAL" | "LINT_TO_REAL"
            | "UINT_TO_REAL" | "UDINT_TO_REAL" | "ULINT_TO_REAL" | "USINT_TO_REAL"
            | "BOOL_TO_REAL" | "INT_TO_LREAL" | "SINT_TO_LREAL" | "DINT_TO_LREAL"
            | "LINT_TO_LREAL" | "REAL_TO_LREAL"
                => Some(Instruction::ToReal),
            "REAL_TO_INT" | "LREAL_TO_INT" | "REAL_TO_DINT" | "LREAL_TO_DINT"
            | "REAL_TO_LINT" | "LREAL_TO_LINT" | "REAL_TO_SINT" | "LREAL_TO_SINT"
            | "BOOL_TO_INT" | "BOOL_TO_DINT" | "BOOL_TO_LINT"
            | "UINT_TO_INT" | "UDINT_TO_DINT" | "ULINT_TO_LINT"
            | "INT_TO_DINT" | "INT_TO_LINT" | "DINT_TO_LINT"
            | "SINT_TO_INT" | "SINT_TO_DINT" | "SINT_TO_LINT"
                => Some(Instruction::ToInt),
            "INT_TO_BOOL" | "REAL_TO_BOOL" | "DINT_TO_BOOL" | "LINT_TO_BOOL"
                => Some(Instruction::ToBool),
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

    fn literal_to_value(&self, lit: &Literal) -> Value {
        match &lit.kind {
            LiteralKind::Integer(v) => Value::Int(*v),
            LiteralKind::Real(v) => Value::Real(*v),
            LiteralKind::Bool(v) => Value::Bool(*v),
            LiteralKind::String(s) => Value::String(s.clone()),
            LiteralKind::Time(s) => Value::Time(parse_time_literal(s)),
            LiteralKind::Date(_) => Value::Time(0),
            LiteralKind::Tod(_) => Value::Time(0),
            LiteralKind::Dt(_) => Value::Time(0),
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

