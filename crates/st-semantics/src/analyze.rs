//! Semantic analyzer: builds symbol table, resolves types, checks types,
//! and collects diagnostics.

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::scope::*;
use crate::types::*;
use st_syntax::ast::*;

/// Result of analyzing a source file.
pub struct AnalysisResult {
    pub symbols: SymbolTable,
    pub diagnostics: Vec<Diagnostic>,
}

/// Analyze a parsed source file.
pub fn analyze(source_file: &SourceFile) -> AnalysisResult {
    analyze_with_native_fbs(source_file, None)
}

/// Analyze a parsed source file, optionally injecting native FB types from a registry.
///
/// When a [`NativeFbRegistry`] is provided, all registered native FB types are
/// added to the symbol table as `SymbolKind::FunctionBlock`, enabling code
/// completion, type checking, and field-access resolution for native FBs.
pub fn analyze_with_native_fbs(
    source_file: &SourceFile,
    registry: Option<&st_comm_api::NativeFbRegistry>,
) -> AnalysisResult {
    let mut analyzer = Analyzer {
        symbols: SymbolTable::new(),
        diagnostics: Vec::new(),
        current_scope: ScopeId(0),
        in_loop: false,
        current_pou_return_type: None,
        current_class: None,
    };
    analyzer.register_intrinsics();
    if let Some(reg) = registry {
        analyzer.register_native_fbs(reg);
    }
    analyzer.analyze_source_file(source_file);
    analyzer.check_unused();
    AnalysisResult {
        symbols: analyzer.symbols,
        diagnostics: analyzer.diagnostics,
    }
}

struct Analyzer {
    symbols: SymbolTable,
    diagnostics: Vec<Diagnostic>,
    current_scope: ScopeId,
    in_loop: bool,
    current_pou_return_type: Option<Ty>,
    /// Name of the class we're currently analyzing (for THIS/SUPER context).
    current_class: Option<String>,
}

impl Analyzer {
    fn error(&mut self, code: DiagnosticCode, msg: impl Into<String>, range: TextRange) {
        self.diagnostics
            .push(Diagnostic::error(code, msg, range));
    }

    fn warning(&mut self, code: DiagnosticCode, msg: impl Into<String>, range: TextRange) {
        self.diagnostics
            .push(Diagnostic::warning(code, msg, range));
    }

    // =========================================================================
    // Pass 1: Register top-level declarations (types, POUs) in global scope
    // Pass 2: Analyze bodies
    // =========================================================================

    /// Register built-in math intrinsic functions so semantic analysis
    /// recognizes them as valid function calls.
    fn register_intrinsics(&mut self) {
        let global = self.symbols.global_scope_id();
        let int_ty = Ty::Elementary(ElementaryType::Int);
        let real_ty = Ty::Elementary(ElementaryType::Real);
        let bool_ty = Ty::Elementary(ElementaryType::Bool);

        // Helper to register a single-arg intrinsic function (scoped to release borrow)
        {
        let mut reg = |name: &str, param_ty: &Ty, ret_ty: &Ty| {
            self.symbols.define(
                global,
                Symbol {
                    name: name.to_string(),
                    ty: ret_ty.clone(),
                    kind: SymbolKind::Function {
                        return_type: ret_ty.clone(),
                        params: vec![ParamDef {
                            name: "IN1".to_string(),
                            ty: param_ty.clone(),
                            var_kind: VarKind::VarInput,
                        }],
                    },
                    range: TextRange::new(0, 0),
                    used: true,
                    assigned: true,
                },
            );
        };

        // Math intrinsics (REAL → REAL)
        for name in ["SQRT", "SIN", "COS", "TAN", "ASIN", "ACOS", "ATAN", "LN", "LOG", "EXP"] {
            reg(name, &real_ty, &real_ty);
        }

        // Type conversion intrinsics
        // Use a permissive input type — the VM handles all conversions dynamically.
        // The return type determines the output.
        // *_TO_REAL / *_TO_LREAL
        for name in [
            "INT_TO_REAL", "SINT_TO_REAL", "DINT_TO_REAL", "LINT_TO_REAL",
            "UINT_TO_REAL", "USINT_TO_REAL", "UDINT_TO_REAL", "ULINT_TO_REAL",
            "BOOL_TO_REAL",
            "INT_TO_LREAL", "SINT_TO_LREAL", "DINT_TO_LREAL", "LINT_TO_LREAL",
            "REAL_TO_LREAL",
        ] {
            reg(name, &int_ty, &real_ty);
        }

        // *_TO_INT (from REAL, BOOL, or other INT sizes)
        for name in [
            "REAL_TO_INT", "LREAL_TO_INT", "REAL_TO_DINT", "LREAL_TO_DINT",
            "REAL_TO_LINT", "LREAL_TO_LINT", "REAL_TO_SINT", "LREAL_TO_SINT",
        ] {
            reg(name, &real_ty, &int_ty);
        }
        for name in [
            "BOOL_TO_INT", "BOOL_TO_DINT", "BOOL_TO_LINT",
        ] {
            reg(name, &bool_ty, &int_ty);
        }
        for name in [
            "UINT_TO_INT", "UDINT_TO_DINT", "ULINT_TO_LINT",
            "INT_TO_DINT", "INT_TO_LINT", "DINT_TO_LINT",
            "SINT_TO_INT", "SINT_TO_DINT", "SINT_TO_LINT",
        ] {
            reg(name, &int_ty, &int_ty);
        }

        // *_TO_BOOL
        for name in ["INT_TO_BOOL", "DINT_TO_BOOL", "LINT_TO_BOOL"] {
            reg(name, &int_ty, &bool_ty);
        }
        reg("REAL_TO_BOOL", &real_ty, &bool_ty);

        // ── TIME conversions ────────────────────────────────────────────
        let time_ty = Ty::Elementary(ElementaryType::Time);

        // TIME_TO_* (TIME → numeric/bool) — value is milliseconds
        for name in [
            "TIME_TO_INT", "TIME_TO_SINT", "TIME_TO_DINT", "TIME_TO_LINT",
            "TIME_TO_UINT", "TIME_TO_USINT", "TIME_TO_UDINT", "TIME_TO_ULINT",
        ] {
            reg(name, &time_ty, &int_ty);
        }
        for name in ["TIME_TO_REAL", "TIME_TO_LREAL"] {
            reg(name, &time_ty, &real_ty);
        }
        reg("TIME_TO_BOOL", &time_ty, &bool_ty);

        // *_TO_TIME (numeric/bool → TIME) — value interpreted as milliseconds
        for name in [
            "INT_TO_TIME", "SINT_TO_TIME", "DINT_TO_TIME", "LINT_TO_TIME",
            "UINT_TO_TIME", "USINT_TO_TIME", "UDINT_TO_TIME", "ULINT_TO_TIME",
        ] {
            reg(name, &int_ty, &time_ty);
        }
        for name in ["REAL_TO_TIME", "LREAL_TO_TIME"] {
            reg(name, &real_ty, &time_ty);
        }
        reg("BOOL_TO_TIME", &bool_ty, &time_ty);

        // ── Overloaded TO_* / ANY_TO_* generic conversions ──────────────
        // These accept any input type; the VM dispatches dynamically.
        // TO_<target> and ANY_TO_<target> are functionally identical.
        // Use Ty::Unknown as parameter type to accept any input (like REF()).
        let any_ty = Ty::Unknown;
        for name in [
            "TO_INT", "TO_SINT", "TO_DINT", "TO_LINT",
            "TO_UINT", "TO_USINT", "TO_UDINT", "TO_ULINT",
            "ANY_TO_INT", "ANY_TO_SINT", "ANY_TO_DINT", "ANY_TO_LINT",
            "ANY_TO_UINT", "ANY_TO_USINT", "ANY_TO_UDINT", "ANY_TO_ULINT",
        ] {
            reg(name, &any_ty, &int_ty);
        }
        for name in [
            "TO_REAL", "TO_LREAL",
            "ANY_TO_REAL", "ANY_TO_LREAL",
        ] {
            reg(name, &any_ty, &real_ty);
        }
        for name in ["TO_BOOL", "ANY_TO_BOOL"] {
            reg(name, &any_ty, &bool_ty);
        }
        for name in ["TO_TIME", "ANY_TO_TIME"] {
            reg(name, &any_ty, &time_ty);
        }

        // ── DATE / TOD / DT conversions ─────────────────────────────────
        let date_ty = Ty::Elementary(ElementaryType::Date);
        let tod_ty = Ty::Elementary(ElementaryType::Tod);
        let dt_ty = Ty::Elementary(ElementaryType::Dt);

        // DATE_TO_* / *_TO_DATE
        for name in [
            "DATE_TO_INT", "DATE_TO_SINT", "DATE_TO_DINT", "DATE_TO_LINT",
            "DATE_TO_UINT", "DATE_TO_USINT", "DATE_TO_UDINT", "DATE_TO_ULINT",
        ] {
            reg(name, &date_ty, &int_ty);
        }
        for name in ["DATE_TO_REAL", "DATE_TO_LREAL"] {
            reg(name, &date_ty, &real_ty);
        }
        reg("DATE_TO_BOOL", &date_ty, &bool_ty);
        for name in [
            "INT_TO_DATE", "SINT_TO_DATE", "DINT_TO_DATE", "LINT_TO_DATE",
            "UINT_TO_DATE", "USINT_TO_DATE", "UDINT_TO_DATE", "ULINT_TO_DATE",
        ] {
            reg(name, &int_ty, &date_ty);
        }
        for name in ["REAL_TO_DATE", "LREAL_TO_DATE"] {
            reg(name, &real_ty, &date_ty);
        }

        // TOD_TO_* / *_TO_TOD
        for name in [
            "TOD_TO_INT", "TOD_TO_SINT", "TOD_TO_DINT", "TOD_TO_LINT",
            "TOD_TO_UINT", "TOD_TO_USINT", "TOD_TO_UDINT", "TOD_TO_ULINT",
        ] {
            reg(name, &tod_ty, &int_ty);
        }
        for name in ["TOD_TO_REAL", "TOD_TO_LREAL"] {
            reg(name, &tod_ty, &real_ty);
        }
        reg("TOD_TO_BOOL", &tod_ty, &bool_ty);
        for name in [
            "INT_TO_TOD", "SINT_TO_TOD", "DINT_TO_TOD", "LINT_TO_TOD",
            "UINT_TO_TOD", "USINT_TO_TOD", "UDINT_TO_TOD", "ULINT_TO_TOD",
        ] {
            reg(name, &int_ty, &tod_ty);
        }
        for name in ["REAL_TO_TOD", "LREAL_TO_TOD"] {
            reg(name, &real_ty, &tod_ty);
        }

        // DT_TO_* / *_TO_DT
        for name in [
            "DT_TO_INT", "DT_TO_SINT", "DT_TO_DINT", "DT_TO_LINT",
            "DT_TO_UINT", "DT_TO_USINT", "DT_TO_UDINT", "DT_TO_ULINT",
        ] {
            reg(name, &dt_ty, &int_ty);
        }
        for name in ["DT_TO_REAL", "DT_TO_LREAL"] {
            reg(name, &dt_ty, &real_ty);
        }
        reg("DT_TO_BOOL", &dt_ty, &bool_ty);
        for name in [
            "INT_TO_DT", "SINT_TO_DT", "DINT_TO_DT", "LINT_TO_DT",
            "UINT_TO_DT", "USINT_TO_DT", "UDINT_TO_DT", "ULINT_TO_DT",
        ] {
            reg(name, &int_ty, &dt_ty);
        }
        for name in ["REAL_TO_DT", "LREAL_TO_DT"] {
            reg(name, &real_ty, &dt_ty);
        }

        // Cross-type extractions (single-arg)
        reg("DT_TO_DATE", &dt_ty, &date_ty);
        reg("DT_TO_TOD", &dt_ty, &tod_ty);
        reg("DATE_TO_DT", &date_ty, &dt_ty);
        reg("TIME_TO_DATE", &time_ty, &date_ty);
        reg("TIME_TO_TOD", &time_ty, &tod_ty);
        reg("TIME_TO_DT", &time_ty, &dt_ty);
        reg("DATE_TO_TIME", &date_ty, &time_ty);
        reg("TOD_TO_TIME", &tod_ty, &time_ty);
        reg("DT_TO_TIME", &dt_ty, &time_ty);

        // DAY_OF_WEEK (DATE → INT)
        reg("DAY_OF_WEEK", &date_ty, &int_ty);

        // TO_DATE / TO_TOD / TO_DT generics
        for name in ["TO_DATE", "ANY_TO_DATE"] {
            reg(name, &any_ty, &date_ty);
        }
        for name in ["TO_TOD", "ANY_TO_TOD"] {
            reg(name, &any_ty, &tod_ty);
        }
        for name in ["TO_DT", "ANY_TO_DT"] {
            reg(name, &any_ty, &dt_ty);
        }
        } // end closure scope

        // ── Two-argument date/time intrinsics ───────────────────────────
        {
        let time_ty = Ty::Elementary(ElementaryType::Time);
        let date_ty = Ty::Elementary(ElementaryType::Date);
        let tod_ty = Ty::Elementary(ElementaryType::Tod);
        let dt_ty = Ty::Elementary(ElementaryType::Dt);
        let int_ty = Ty::Elementary(ElementaryType::Int);

        let mut reg2 = |name: &str, p1_ty: &Ty, p2_ty: &Ty, ret_ty: &Ty| {
            self.symbols.define(
                global,
                Symbol {
                    name: name.to_string(),
                    ty: ret_ty.clone(),
                    kind: SymbolKind::Function {
                        return_type: ret_ty.clone(),
                        params: vec![
                            ParamDef { name: "IN1".to_string(), ty: p1_ty.clone(), var_kind: VarKind::VarInput },
                            ParamDef { name: "IN2".to_string(), ty: p2_ty.clone(), var_kind: VarKind::VarInput },
                        ],
                    },
                    range: TextRange::new(0, 0),
                    used: true,
                    assigned: true,
                },
            );
        };

        // Date/time arithmetic
        reg2("ADD_TOD_TIME", &tod_ty, &time_ty, &tod_ty);
        reg2("ADD_DT_TIME", &dt_ty, &time_ty, &dt_ty);
        reg2("SUB_TOD_TIME", &tod_ty, &time_ty, &tod_ty);
        reg2("SUB_DATE_DATE", &date_ty, &date_ty, &time_ty);
        reg2("SUB_TOD_TOD", &tod_ty, &tod_ty, &time_ty);
        reg2("SUB_DT_TIME", &dt_ty, &time_ty, &dt_ty);
        reg2("SUB_DT_DT", &dt_ty, &dt_ty, &time_ty);
        reg2("CONCAT_DATE_TOD", &date_ty, &tod_ty, &dt_ty);
        reg2("MULTIME", &time_ty, &int_ty, &time_ty);
        reg2("DIVTIME", &time_ty, &int_ty, &time_ty);
        }

        // REF() intrinsic — takes any variable, returns a reference
        self.symbols.define(
            global,
            Symbol {
                name: "REF".to_string(),
                ty: Ty::Unknown, // return type depends on argument
                kind: SymbolKind::Function {
                    return_type: Ty::Unknown,
                    params: vec![ParamDef {
                        name: "IN1".to_string(),
                        ty: Ty::Unknown, // accepts any type
                        var_kind: VarKind::VarInput,
                    }],
                },
                range: TextRange::new(0, 0),
                used: true,
                assigned: true,
            },
        );

        // System time intrinsic (no args → TIME)
        let time_ty = Ty::Elementary(ElementaryType::Time);
        self.symbols.define(
            global,
            Symbol {
                name: "SYSTEM_TIME".to_string(),
                ty: time_ty.clone(),
                kind: SymbolKind::Function {
                    return_type: time_ty,
                    params: vec![],
                },
                range: TextRange::new(0, 0),
                used: true,
                assigned: true,
            },
        );
    }

    /// Register native FB types from a [`NativeFbRegistry`] into the global scope.
    ///
    /// Each native FB becomes a `SymbolKind::FunctionBlock` with its VAR_INPUT
    /// fields as `params` and VAR fields as `outputs` (so they're accessible
    /// via `instance.field` dot notation through the existing field-access code).
    fn register_native_fbs(&mut self, registry: &st_comm_api::NativeFbRegistry) {
        use st_comm_api::native_fb::NativeFbVarKind;

        let global = self.symbols.global_scope_id();

        for native_fb in registry.all() {
            let layout = native_fb.layout();
            let mut params = Vec::new();
            let mut outputs = Vec::new();

            for field in &layout.fields {
                let ty = field_data_type_to_semantic_ty(field.data_type);
                let pd = ParamDef {
                    name: field.name.clone(),
                    ty,
                    var_kind: match field.var_kind {
                        NativeFbVarKind::VarInput => VarKind::VarInput,
                        NativeFbVarKind::Var => VarKind::Var,
                    },
                };
                match field.var_kind {
                    NativeFbVarKind::VarInput => params.push(pd),
                    NativeFbVarKind::Var => outputs.push(pd),
                }
            }

            self.symbols.define(
                global,
                Symbol {
                    name: layout.type_name.clone(),
                    ty: Ty::FunctionBlock {
                        name: layout.type_name.clone(),
                    },
                    kind: SymbolKind::FunctionBlock { params, outputs },
                    range: TextRange::new(0, 0),
                    used: true,
                    assigned: true,
                },
            );
        }
    }

    fn analyze_source_file(&mut self, sf: &SourceFile) {
        // Pass 1: register all top-level names so forward references work
        for item in &sf.items {
            self.register_top_level(item);
        }
        // Pass 2: analyze bodies
        for item in &sf.items {
            self.analyze_top_level(item);
        }
    }

    fn register_top_level(&mut self, item: &TopLevelItem) {
        let global = self.symbols.global_scope_id();
        match item {
            TopLevelItem::TypeDeclaration(td) => {
                for def in &td.definitions {
                    let ty = self.resolve_type_def_kind(&def.ty, &def.name.name);
                    let prev = self.symbols.define(
                        global,
                        Symbol {
                            name: def.name.name.clone(),
                            ty,
                            kind: SymbolKind::Type,
                            range: def.range,
                            used: false,
                            assigned: false,
                        },
                    );
                    if prev.is_some() {
                        self.error(
                            DiagnosticCode::DuplicateDeclaration,
                            format!("duplicate type declaration '{}'", def.name.name),
                            def.name.range,
                        );
                    }
                }
            }
            TopLevelItem::Function(f) => {
                let return_type = self.resolve_data_type(&f.return_type);
                let params = self.collect_params(&f.var_blocks);
                let prev = self.symbols.define(
                    global,
                    Symbol {
                        name: f.name.name.clone(),
                        ty: return_type.clone(),
                        kind: SymbolKind::Function {
                            return_type,
                            params,
                        },
                        range: f.range,
                        used: false,
                        assigned: false,
                    },
                );
                if prev.is_some() {
                    self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("duplicate function declaration '{}'", f.name.name),
                        f.name.range,
                    );
                }
            }
            TopLevelItem::FunctionBlock(fb) => {
                let params = self.collect_params_by_kind(&fb.var_blocks, |k| {
                    matches!(k, VarKind::VarInput | VarKind::VarInOut)
                });
                let outputs = self.collect_params_by_kind(&fb.var_blocks, |k| {
                    matches!(k, VarKind::VarOutput)
                });
                let prev = self.symbols.define(
                    global,
                    Symbol {
                        name: fb.name.name.clone(),
                        ty: Ty::FunctionBlock {
                            name: fb.name.name.clone(),
                        },
                        kind: SymbolKind::FunctionBlock { params, outputs },
                        range: fb.range,
                        used: false,
                        assigned: false,
                    },
                );
                if prev.is_some() {
                    self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!(
                            "duplicate function block declaration '{}'",
                            fb.name.name
                        ),
                        fb.name.range,
                    );
                }
            }
            TopLevelItem::Class(cls) => {
                let params = self.collect_params_by_kind(&cls.var_blocks, |k| {
                    matches!(k, VarKind::VarInput | VarKind::VarInOut)
                });
                let outputs = self.collect_params_by_kind(&cls.var_blocks, |k| {
                    matches!(k, VarKind::VarOutput)
                });
                let vars = self.collect_params_by_kind(&cls.var_blocks, |k| {
                    matches!(k, VarKind::Var | VarKind::VarTemp)
                });
                let methods: Vec<MethodDef> = cls.methods.iter().map(|m| {
                    let m_params = self.collect_params_by_kind(&m.var_blocks, |k| {
                        matches!(k, VarKind::VarInput | VarKind::VarInOut)
                    });
                    let ret_ty = m.return_type.as_ref()
                        .map(|dt| self.resolve_data_type(dt))
                        .unwrap_or(Ty::Void);
                    MethodDef {
                        name: m.name.name.clone(),
                        return_type: ret_ty,
                        params: m_params,
                        access: match m.access {
                            AccessSpecifier::Public => AccessLevel::Public,
                            AccessSpecifier::Private => AccessLevel::Private,
                            AccessSpecifier::Protected => AccessLevel::Protected,
                            AccessSpecifier::Internal => AccessLevel::Internal,
                        },
                        is_abstract: m.is_abstract,
                        is_final: m.is_final,
                        is_override: m.is_override,
                    }
                }).collect();

                let prev = self.symbols.define(
                    global,
                    Symbol {
                        name: cls.name.name.clone(),
                        ty: Ty::Class { name: cls.name.name.clone() },
                        kind: SymbolKind::Class {
                            params,
                            outputs,
                            vars,
                            methods,
                            base_class: cls.base_class.clone(),
                            interfaces: cls.interfaces.clone(),
                            is_abstract: cls.is_abstract,
                            is_final: cls.is_final,
                        },
                        range: cls.range,
                        used: false,
                        assigned: false,
                    },
                );
                if prev.is_some() {
                    self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("duplicate class declaration '{}'", cls.name.name),
                        cls.name.range,
                    );
                }
            }
            TopLevelItem::Interface(iface) => {
                let methods: Vec<MethodDef> = iface.methods.iter().map(|m| {
                    let m_params = self.collect_params_by_kind(&m.var_blocks, |k| {
                        matches!(k, VarKind::VarInput | VarKind::VarInOut)
                    });
                    let ret_ty = m.return_type.as_ref()
                        .map(|dt| self.resolve_data_type(dt))
                        .unwrap_or(Ty::Void);
                    MethodDef {
                        name: m.name.name.clone(),
                        return_type: ret_ty,
                        params: m_params,
                        access: AccessLevel::Public,
                        is_abstract: false,
                        is_final: false,
                        is_override: false,
                    }
                }).collect();

                let prev = self.symbols.define(
                    global,
                    Symbol {
                        name: iface.name.name.clone(),
                        ty: Ty::Interface { name: iface.name.name.clone() },
                        kind: SymbolKind::Interface {
                            methods,
                            base_interfaces: iface.base_interfaces.clone(),
                        },
                        range: iface.range,
                        used: false,
                        assigned: false,
                    },
                );
                if prev.is_some() {
                    self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("duplicate interface declaration '{}'", iface.name.name),
                        iface.name.range,
                    );
                }
            }
            TopLevelItem::Program(p) => {
                let params = self.collect_params(&p.var_blocks);
                let prev = self.symbols.define(
                    global,
                    Symbol {
                        name: p.name.name.clone(),
                        ty: Ty::Void,
                        kind: SymbolKind::Program { params },
                        range: p.range,
                        used: false,
                        assigned: false,
                    },
                );
                if prev.is_some() {
                    self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("duplicate program declaration '{}'", p.name.name),
                        p.name.range,
                    );
                }
            }
            TopLevelItem::GlobalVarDeclaration(vb) => {
                self.define_var_block(global, vb);
            }
        }
    }

    fn analyze_top_level(&mut self, item: &TopLevelItem) {
        match item {
            TopLevelItem::Program(p) => {
                let scope = self
                    .symbols
                    .create_scope(self.symbols.global_scope_id(), p.name.name.clone());
                let saved = self.current_scope;
                self.current_scope = scope;
                self.current_pou_return_type = None;
                for vb in &p.var_blocks {
                    self.define_var_block(scope, vb);
                }
                self.analyze_statements(&p.body);
                self.current_scope = saved;
            }
            TopLevelItem::Function(f) => {
                let scope = self
                    .symbols
                    .create_scope(self.symbols.global_scope_id(), f.name.name.clone());
                let saved = self.current_scope;
                self.current_scope = scope;
                let ret_ty = self.resolve_data_type(&f.return_type);
                self.current_pou_return_type = Some(ret_ty);
                // Define the function name as a variable (for `FuncName := result`)
                let return_type = self.resolve_data_type(&f.return_type);
                self.symbols.define(
                    scope,
                    Symbol {
                        name: f.name.name.clone(),
                        ty: return_type,
                        kind: SymbolKind::Variable(VarKind::Var),
                        range: f.name.range,
                        used: true, // don't warn about unused
                        assigned: false,
                    },
                );
                for vb in &f.var_blocks {
                    self.define_var_block(scope, vb);
                }
                self.analyze_statements(&f.body);
                self.current_scope = saved;
                self.current_pou_return_type = None;
            }
            TopLevelItem::FunctionBlock(fb) => {
                let scope = self
                    .symbols
                    .create_scope(self.symbols.global_scope_id(), fb.name.name.clone());
                let saved = self.current_scope;
                self.current_scope = scope;
                self.current_pou_return_type = None;
                for vb in &fb.var_blocks {
                    self.define_var_block(scope, vb);
                }
                self.analyze_statements(&fb.body);
                self.current_scope = saved;
            }
            TopLevelItem::Class(cls) => {
                self.analyze_class(cls);
            }
            TopLevelItem::Interface(iface) => {
                self.analyze_interface(iface);
            }
            TopLevelItem::TypeDeclaration(_) | TopLevelItem::GlobalVarDeclaration(_) => {
                // Already handled in pass 1
            }
        }
    }

    // =========================================================================
    // Class / Interface analysis
    // =========================================================================

    fn analyze_class(&mut self, cls: &ClassDecl) {
        let scope = self
            .symbols
            .create_scope(self.symbols.global_scope_id(), cls.name.name.clone());
        let saved_scope = self.current_scope;
        let saved_class = self.current_class.clone();
        self.current_scope = scope;
        self.current_class = Some(cls.name.name.clone());
        self.current_pou_return_type = None;

        // Validate base class
        if let Some(ref base_name) = cls.base_class {
            match self.symbols.resolve_class(base_name) {
                Some(sym) => {
                    if let SymbolKind::Class { is_final, .. } = &sym.kind {
                        if *is_final {
                            self.error(
                                DiagnosticCode::CannotExtendFinalClass,
                                format!("cannot extend FINAL class '{base_name}'"),
                                cls.name.range,
                            );
                        }
                    }
                }
                None => {
                    self.error(
                        DiagnosticCode::UndeclaredType,
                        format!("undeclared base class '{base_name}'"),
                        cls.name.range,
                    );
                }
            }
        }

        // Validate interfaces
        for iface_name in &cls.interfaces {
            if self.symbols.resolve_interface(iface_name).is_none() {
                self.error(
                    DiagnosticCode::UndeclaredType,
                    format!("undeclared interface '{iface_name}'"),
                    cls.name.range,
                );
            }
        }

        // Check interface conformance — all interface methods must be implemented
        for iface_name in &cls.interfaces {
            if let Some(sym) = self.symbols.resolve_interface(iface_name) {
                if let SymbolKind::Interface { methods: iface_methods, .. } = &sym.kind {
                    let iface_methods = iface_methods.clone();
                    for im in &iface_methods {
                        let implemented = cls.methods.iter().any(|m| {
                            m.name.name.eq_ignore_ascii_case(&im.name)
                        });
                        if !implemented {
                            self.error(
                                DiagnosticCode::InterfaceNotImplemented,
                                format!(
                                    "class '{}' does not implement method '{}' from interface '{}'",
                                    cls.name.name, im.name, iface_name
                                ),
                                cls.name.range,
                            );
                        }
                    }
                }
            }
        }

        // Check abstract methods in non-abstract class
        if !cls.is_abstract {
            for m in &cls.methods {
                if m.is_abstract {
                    self.error(
                        DiagnosticCode::AbstractMethodInNonAbstractClass,
                        format!(
                            "abstract method '{}' in non-abstract class '{}'",
                            m.name.name, cls.name.name
                        ),
                        m.name.range,
                    );
                }
            }
        }

        // Define inherited var blocks from parent classes (walk hierarchy)
        self.define_inherited_vars(scope, &cls.base_class);

        // Define var blocks in class scope
        for vb in &cls.var_blocks {
            self.define_var_block(scope, vb);
        }

        // Analyze methods
        for method in &cls.methods {
            self.analyze_method(method, &cls.name.name, &cls.base_class);
        }

        // Analyze properties
        for prop in &cls.properties {
            self.analyze_property(prop);
        }

        self.current_scope = saved_scope;
        self.current_class = saved_class;
    }

    fn analyze_method(
        &mut self,
        method: &MethodDecl,
        class_name: &str,
        base_class: &Option<String>,
    ) {
        let method_scope = self
            .symbols
            .create_scope(self.current_scope, method.name.name.clone());
        let saved = self.current_scope;
        self.current_scope = method_scope;

        let ret_ty = method.return_type.as_ref()
            .map(|dt| self.resolve_data_type(dt));
        self.current_pou_return_type = ret_ty.clone();

        // Define return variable if method has return type
        if let Some(ref ret) = ret_ty {
            self.symbols.define(
                method_scope,
                Symbol {
                    name: method.name.name.clone(),
                    ty: ret.clone(),
                    kind: SymbolKind::Variable(VarKind::Var),
                    range: method.name.range,
                    used: true,
                    assigned: false,
                },
            );
        }

        // Validate OVERRIDE
        if method.is_override {
            if let Some(base_name) = base_class {
                let base_has_method = self.symbols.resolve_class(base_name)
                    .and_then(|sym| {
                        if let SymbolKind::Class { methods, .. } = &sym.kind {
                            methods.iter().find(|m| m.name.eq_ignore_ascii_case(&method.name.name)).cloned()
                        } else {
                            None
                        }
                    });
                match base_has_method {
                    Some(base_method) => {
                        if base_method.is_final {
                            self.error(
                                DiagnosticCode::CannotOverrideFinalMethod,
                                format!(
                                    "cannot override FINAL method '{}' from base class '{}'",
                                    method.name.name, base_name
                                ),
                                method.name.range,
                            );
                        }
                    }
                    None => {
                        self.error(
                            DiagnosticCode::InvalidOverride,
                            format!(
                                "method '{}' marked OVERRIDE but no matching method in base class '{}'",
                                method.name.name, base_name
                            ),
                            method.name.range,
                        );
                    }
                }
            } else {
                self.error(
                    DiagnosticCode::InvalidOverride,
                    format!(
                        "method '{}' marked OVERRIDE but class '{}' has no base class",
                        method.name.name, class_name
                    ),
                    method.name.range,
                );
            }
        }

        for vb in &method.var_blocks {
            self.define_var_block(method_scope, vb);
        }

        // Abstract methods have no body to analyze
        if !method.is_abstract {
            self.analyze_statements(&method.body);
        }

        self.current_scope = saved;
        self.current_pou_return_type = None;
    }

    fn analyze_property(&mut self, prop: &PropertyDecl) {
        let prop_ty = self.resolve_data_type(&prop.ty);

        if let Some(ref getter) = prop.get_body {
            let get_scope = self
                .symbols
                .create_scope(self.current_scope, format!("{}.GET", prop.name.name));
            let saved = self.current_scope;
            self.current_scope = get_scope;
            // Define the property name as a return variable
            self.symbols.define(
                get_scope,
                Symbol {
                    name: prop.name.name.clone(),
                    ty: prop_ty.clone(),
                    kind: SymbolKind::Variable(VarKind::Var),
                    range: prop.name.range,
                    used: true,
                    assigned: false,
                },
            );
            for vb in &getter.var_blocks {
                self.define_var_block(get_scope, vb);
            }
            self.analyze_statements(&getter.body);
            self.current_scope = saved;
        }

        if let Some(ref setter) = prop.set_body {
            let set_scope = self
                .symbols
                .create_scope(self.current_scope, format!("{}.SET", prop.name.name));
            let saved = self.current_scope;
            self.current_scope = set_scope;
            // Define implicit input variable for setter
            self.symbols.define(
                set_scope,
                Symbol {
                    name: prop.name.name.clone(),
                    ty: prop_ty.clone(),
                    kind: SymbolKind::Variable(VarKind::VarInput),
                    range: prop.name.range,
                    used: false,
                    assigned: true,
                },
            );
            for vb in &setter.var_blocks {
                self.define_var_block(set_scope, vb);
            }
            self.analyze_statements(&setter.body);
            self.current_scope = saved;
        }
    }

    /// Define variables from parent classes into a class scope.
    /// Walks up the inheritance chain and defines all ancestor vars.
    fn define_inherited_vars(&mut self, scope_id: ScopeId, base_class: &Option<String>) {
        // Collect the chain bottom-up, then define top-down so overrides work correctly
        let mut chain = Vec::new();
        let mut current = base_class.clone();
        while let Some(ref cls_name) = current {
            if let Some(sym) = self.symbols.resolve_class(cls_name) {
                if let SymbolKind::Class { params, outputs, vars, base_class: parent, .. } = &sym.kind {
                    chain.push((params.clone(), outputs.clone(), vars.clone()));
                    current = parent.clone();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        // Define from root ancestor down
        for (params, outputs, vars) in chain.into_iter().rev() {
            for p in params.iter().chain(outputs.iter()).chain(vars.iter()) {
                // Only define if not already defined (avoid duplicates from diamond)
                if self.symbols.scope(scope_id).lookup_local(&p.name).is_none() {
                    self.symbols.define(scope_id, Symbol {
                        name: p.name.clone(),
                        ty: p.ty.clone(),
                        kind: SymbolKind::Variable(p.var_kind),
                        range: TextRange::new(0, 0),
                        used: true,
                        assigned: true,
                    });
                }
            }
        }
    }

    fn analyze_interface(&mut self, iface: &InterfaceDecl) {
        // Validate base interfaces exist
        for base_name in &iface.base_interfaces {
            if self.symbols.resolve_interface(base_name).is_none() {
                self.error(
                    DiagnosticCode::UndeclaredType,
                    format!("undeclared base interface '{base_name}'"),
                    iface.name.range,
                );
            }
        }

        // Validate method prototypes (just check types resolve)
        for method in &iface.methods {
            if let Some(ref ret_dt) = method.return_type {
                self.resolve_data_type(ret_dt);
            }
            for vb in &method.var_blocks {
                for decl in &vb.declarations {
                    self.resolve_data_type(&decl.ty);
                }
            }
        }
    }

    // =========================================================================
    // Variable block registration
    // =========================================================================

    fn define_var_block(&mut self, scope_id: ScopeId, vb: &VarBlock) {
        let is_constant = vb.qualifiers.contains(&VarQualifier::Constant);
        for decl in &vb.declarations {
            let ty = self.resolve_data_type(&decl.ty);
            for name_id in &decl.names {
                // Check for shadowing
                if let Some((parent_scope, _existing)) =
                    self.symbols.resolve(scope_id, &name_id.name)
                {
                    if parent_scope != scope_id {
                        self.warning(
                            DiagnosticCode::ShadowedVariable,
                            format!("'{}' shadows a variable in an outer scope", name_id.name),
                            name_id.range,
                        );
                    }
                }

                let prev = self.symbols.define(
                    scope_id,
                    Symbol {
                        name: name_id.name.clone(),
                        ty: ty.clone(),
                        kind: SymbolKind::Variable(vb.kind),
                        range: name_id.range,
                        used: false,
                        assigned: decl.initial_value.is_some()
                            || matches!(
                                vb.kind,
                                VarKind::VarInput | VarKind::VarInOut | VarKind::VarExternal
                            ),
                    },
                );
                if prev.is_some() {
                    self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("duplicate variable declaration '{}'", name_id.name),
                        name_id.range,
                    );
                }
            }
            // Type-check initial value if present
            if let Some(init_expr) = &decl.initial_value {
                let init_ty = self.check_expression(init_expr);
                if !is_type_compatible(&init_ty, &ty)
                    && !matches!(init_ty, Ty::Unknown)
                    && !literal_fits_in_target(init_expr, &ty)
                {
                    self.error(
                        DiagnosticCode::TypeMismatchAssignment,
                        format!(
                            "cannot initialize '{}' variable with '{}' value",
                            ty.display_name(),
                            init_ty.display_name()
                        ),
                        init_expr.range(),
                    );
                }
            }
            // Check constant has initial value
            if is_constant && decl.initial_value.is_none() {
                for name_id in &decl.names {
                    self.error(
                        DiagnosticCode::AssignmentToConstant,
                        format!("CONSTANT '{}' must have an initial value", name_id.name),
                        name_id.range,
                    );
                }
            }
        }
    }

    // =========================================================================
    // Statement analysis
    // =========================================================================

    fn analyze_statements(&mut self, stmts: &[Statement]) {
        let mut seen_return = false;
        for stmt in stmts {
            if seen_return {
                self.warning(
                    DiagnosticCode::DeadCode,
                    "unreachable code after RETURN",
                    stmt.range(),
                );
            }
            self.analyze_statement(stmt);
            if matches!(stmt, Statement::Return(_)) {
                seen_return = true;
            }
        }
    }

    fn analyze_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Assignment(a) => self.analyze_assignment(a),
            Statement::FunctionCall(fc) => {
                self.check_function_call(fc);
            }
            Statement::If(if_stmt) => self.analyze_if(if_stmt),
            Statement::Case(case_stmt) => self.analyze_case(case_stmt),
            Statement::For(for_stmt) => self.analyze_for(for_stmt),
            Statement::While(w) => self.analyze_while(w),
            Statement::Repeat(r) => self.analyze_repeat(r),
            Statement::Return(_) => {}
            Statement::Exit(range) => {
                if !self.in_loop {
                    self.error(
                        DiagnosticCode::ExitOutsideLoop,
                        "EXIT statement is only allowed inside a loop",
                        *range,
                    );
                }
            }
            Statement::Empty(_) => {}
        }
    }

    fn analyze_assignment(&mut self, a: &AssignmentStmt) {
        let target_ty = self.check_variable_access_for_write(&a.target);
        let value_ty = self.check_expression(&a.value);

        if !matches!(target_ty, Ty::Unknown)
            && !matches!(value_ty, Ty::Unknown)
            && !is_type_compatible(&value_ty, &target_ty)
            && !literal_fits_in_target(&a.value, &target_ty)
        {
            self.error(
                DiagnosticCode::TypeMismatchAssignment,
                format!(
                    "cannot assign '{}' to '{}'",
                    value_ty.display_name(),
                    target_ty.display_name()
                ),
                a.range,
            );
        }
    }

    fn check_variable_access_for_write(&mut self, va: &VariableAccess) -> Ty {
        let ty = self.check_variable_access(va);

        // Check the root variable for writability
        if let Some(AccessPart::Identifier(id)) = va.parts.first() {
            if let Some((_scope_id, sym)) = self.symbols.resolve(self.current_scope, &id.name) {
                match &sym.kind {
                    SymbolKind::Variable(VarKind::VarInput) => {
                        // In ST, assigning to VAR_INPUT inside the POU is allowed
                        // (it's a local copy), but assigning from outside is not.
                        // We'll allow it here since we're analyzing the POU body.
                    }
                    SymbolKind::Variable(vk) => {
                        // Check for CONSTANT qualifier
                        let scope = self.symbols.scope(self.current_scope);
                        if let Some(sym) = scope.lookup_local(&id.name) {
                            if matches!(sym.kind, SymbolKind::Variable(_)) {
                                // We'd need qualifier info; check by looking at the
                                // parent scope for CONSTANT. For now, we check the
                                // symbol's scope for constant blocks. This is simplified.
                            }
                        }
                        let _ = vk; // suppress unused warning
                    }
                    _ => {}
                }
                self.symbols.mark_assigned(self.current_scope, &id.name);
            }
        }
        ty
    }

    fn analyze_if(&mut self, if_stmt: &IfStmt) {
        let cond_ty = self.check_expression(&if_stmt.condition);
        self.expect_bool(&cond_ty, if_stmt.condition.range());
        self.analyze_statements(&if_stmt.then_body);
        for elsif in &if_stmt.elsif_clauses {
            let cond_ty = self.check_expression(&elsif.condition);
            self.expect_bool(&cond_ty, elsif.condition.range());
            self.analyze_statements(&elsif.body);
        }
        if let Some(else_body) = &if_stmt.else_body {
            self.analyze_statements(else_body);
        }
    }

    fn analyze_case(&mut self, case_stmt: &CaseStmt) {
        let expr_ty = self.check_expression(&case_stmt.expression);

        for branch in &case_stmt.branches {
            for selector in &branch.selectors {
                match selector {
                    CaseSelector::Single(expr) => {
                        let sel_ty = self.check_expression(expr);
                        if !matches!(sel_ty, Ty::Unknown)
                            && !matches!(expr_ty, Ty::Unknown)
                            && !is_type_compatible(&sel_ty, &expr_ty)
                        {
                            self.error(
                                DiagnosticCode::CaseSelectorTypeMismatch,
                                format!(
                                    "case selector type '{}' incompatible with expression type '{}'",
                                    sel_ty.display_name(),
                                    expr_ty.display_name()
                                ),
                                expr.range(),
                            );
                        }
                    }
                    CaseSelector::Range(lo, hi) => {
                        let lo_ty = self.check_expression(lo);
                        let hi_ty = self.check_expression(hi);
                        for (ty, range) in [(&lo_ty, lo.range()), (&hi_ty, hi.range())] {
                            if !matches!(ty, Ty::Unknown)
                                && !matches!(expr_ty, Ty::Unknown)
                                && !is_type_compatible(ty, &expr_ty)
                            {
                                self.error(
                                    DiagnosticCode::CaseSelectorTypeMismatch,
                                    format!(
                                        "case range type '{}' incompatible with expression type '{}'",
                                        ty.display_name(),
                                        expr_ty.display_name()
                                    ),
                                    range,
                                );
                            }
                        }
                    }
                }
            }
            self.analyze_statements(&branch.body);
        }
        if let Some(else_body) = &case_stmt.else_body {
            self.analyze_statements(else_body);
        }
    }

    fn analyze_for(&mut self, for_stmt: &ForStmt) {
        // Check loop variable exists and is integer
        if let Some((_sid, sym)) = self.symbols.resolve(self.current_scope, &for_stmt.variable.name)
        {
            if !sym.ty.is_integer() && !matches!(sym.ty, Ty::Unknown) {
                self.error(
                    DiagnosticCode::ForVariableNotInteger,
                    format!(
                        "FOR variable '{}' must be an integer type, found '{}'",
                        for_stmt.variable.name,
                        sym.ty.display_name()
                    ),
                    for_stmt.variable.range,
                );
            }
            self.symbols
                .mark_used(self.current_scope, &for_stmt.variable.name);
            self.symbols
                .mark_assigned(self.current_scope, &for_stmt.variable.name);
        } else {
            self.error(
                DiagnosticCode::UndeclaredVariable,
                format!("undeclared variable '{}'", for_stmt.variable.name),
                for_stmt.variable.range,
            );
        }

        let from_ty = self.check_expression(&for_stmt.from);
        let to_ty = self.check_expression(&for_stmt.to);
        if !from_ty.is_integer() && !matches!(from_ty, Ty::Unknown) {
            self.error(
                DiagnosticCode::TypeMismatch,
                format!("FOR 'from' must be integer, found '{}'", from_ty.display_name()),
                for_stmt.from.range(),
            );
        }
        if !to_ty.is_integer() && !matches!(to_ty, Ty::Unknown) {
            self.error(
                DiagnosticCode::TypeMismatch,
                format!("FOR 'to' must be integer, found '{}'", to_ty.display_name()),
                for_stmt.to.range(),
            );
        }
        if let Some(by_expr) = &for_stmt.by {
            let by_ty = self.check_expression(by_expr);
            if !by_ty.is_integer() && !matches!(by_ty, Ty::Unknown) {
                self.error(
                    DiagnosticCode::TypeMismatch,
                    format!("FOR 'by' must be integer, found '{}'", by_ty.display_name()),
                    by_expr.range(),
                );
            }
        }

        let saved = self.in_loop;
        self.in_loop = true;
        self.analyze_statements(&for_stmt.body);
        self.in_loop = saved;
    }

    fn analyze_while(&mut self, w: &WhileStmt) {
        let cond_ty = self.check_expression(&w.condition);
        self.expect_bool(&cond_ty, w.condition.range());
        let saved = self.in_loop;
        self.in_loop = true;
        self.analyze_statements(&w.body);
        self.in_loop = saved;
    }

    fn analyze_repeat(&mut self, r: &RepeatStmt) {
        let saved = self.in_loop;
        self.in_loop = true;
        self.analyze_statements(&r.body);
        self.in_loop = saved;
        let cond_ty = self.check_expression(&r.condition);
        self.expect_bool(&cond_ty, r.condition.range());
    }

    // =========================================================================
    // Expression type checking
    // =========================================================================

    fn check_expression(&mut self, expr: &Expression) -> Ty {
        match expr {
            Expression::Literal(lit) => self.literal_type(lit),
            Expression::Variable(va) => self.check_variable_access(va),
            Expression::FunctionCall(fc) => self.check_function_call(fc),
            Expression::Unary(u) => self.check_unary(u),
            Expression::Binary(b) => self.check_binary(b),
            Expression::Parenthesized(inner) => self.check_expression(inner),
            Expression::This(range) => {
                if let Some(ref class_name) = self.current_class {
                    Ty::Class { name: class_name.clone() }
                } else {
                    self.error(
                        DiagnosticCode::InvalidThisContext,
                        "THIS is only valid inside a class method",
                        *range,
                    );
                    Ty::Unknown
                }
            }
            Expression::Super(range) => {
                if let Some(ref class_name) = self.current_class {
                    // Look up base class
                    let base = self.symbols.resolve_class(class_name)
                        .and_then(|sym| {
                            if let SymbolKind::Class { base_class, .. } = &sym.kind {
                                base_class.clone()
                            } else {
                                None
                            }
                        });
                    if let Some(base_name) = base {
                        Ty::Class { name: base_name }
                    } else {
                        self.error(
                            DiagnosticCode::InvalidSuperContext,
                            "SUPER requires a base class (no EXTENDS clause)",
                            *range,
                        );
                        Ty::Unknown
                    }
                } else {
                    self.error(
                        DiagnosticCode::InvalidSuperContext,
                        "SUPER is only valid inside a class method",
                        *range,
                    );
                    Ty::Unknown
                }
            }
        }
    }

    fn literal_type(&self, lit: &Literal) -> Ty {
        match &lit.kind {
            LiteralKind::Integer(_) => Ty::Elementary(ElementaryType::Int),
            LiteralKind::Real(_) => Ty::Elementary(ElementaryType::Real),
            LiteralKind::String(_) => Ty::String {
                wide: false,
                max_len: None,
            },
            LiteralKind::Bool(_) => Ty::Elementary(ElementaryType::Bool),
            LiteralKind::Time(_) => Ty::Elementary(ElementaryType::Time),
            LiteralKind::Date(_) => Ty::Elementary(ElementaryType::Date),
            LiteralKind::Tod(_) => Ty::Elementary(ElementaryType::Tod),
            LiteralKind::Dt(_) => Ty::Elementary(ElementaryType::Dt),
            LiteralKind::Null => Ty::Unknown, // NULL is compatible with any REF_TO
            LiteralKind::Typed { ty, .. } => Ty::Elementary(*ty),
        }
    }

    fn check_variable_access(&mut self, va: &VariableAccess) -> Ty {
        let mut current_ty = Ty::Unknown;

        for (i, part) in va.parts.iter().enumerate() {
            match part {
                AccessPart::Identifier(id) => {
                    if i == 0 {
                        // Root variable lookup
                        if let Some((_sid, sym)) =
                            self.symbols.resolve(self.current_scope, &id.name)
                        {
                            current_ty = sym.ty.clone();
                            self.symbols.mark_used(self.current_scope, &id.name);
                        } else {
                            self.error(
                                DiagnosticCode::UndeclaredVariable,
                                format!("undeclared variable '{}'", id.name),
                                id.range,
                            );
                            return Ty::Unknown;
                        }
                    } else {
                        // Field access
                        match current_ty.resolved() {
                            Ty::Struct { fields, .. } => {
                                if let Some(field) = fields
                                    .iter()
                                    .find(|f| f.name.eq_ignore_ascii_case(&id.name))
                                {
                                    current_ty = field.ty.clone();
                                } else {
                                    self.error(
                                        DiagnosticCode::NoSuchField,
                                        format!(
                                            "no field '{}' on type '{}'",
                                            id.name,
                                            current_ty.display_name()
                                        ),
                                        id.range,
                                    );
                                    return Ty::Unknown;
                                }
                            }
                            Ty::FunctionBlock { name } => {
                                // FB instance field access — look up the FB's outputs/vars
                                if let Some(sym) = self.symbols.resolve_pou(name) {
                                    if let SymbolKind::FunctionBlock { outputs, params } =
                                        &sym.kind
                                    {
                                        let all_params: Vec<_> =
                                            params.iter().chain(outputs.iter()).collect();
                                        if let Some(p) = all_params
                                            .iter()
                                            .find(|p| p.name.eq_ignore_ascii_case(&id.name))
                                        {
                                            current_ty = p.ty.clone();
                                        } else {
                                            self.error(
                                                DiagnosticCode::NoSuchField,
                                                format!(
                                                    "no member '{}' on function block '{}'",
                                                    id.name, name
                                                ),
                                                id.range,
                                            );
                                            return Ty::Unknown;
                                        }
                                    }
                                } else {
                                    return Ty::Unknown;
                                }
                            }
                            Ty::Class { name } => {
                                // Class instance field/method access
                                if let Some(sym) = self.symbols.resolve_class(name) {
                                    if let SymbolKind::Class { params, outputs, methods, .. } = &sym.kind {
                                        // Check methods first
                                        if let Some(m) = methods.iter().find(|m| m.name.eq_ignore_ascii_case(&id.name)) {
                                            current_ty = m.return_type.clone();
                                        }
                                        // Then check fields
                                        else {
                                            let all_params: Vec<_> = params.iter().chain(outputs.iter()).collect();
                                            if let Some(p) = all_params.iter().find(|p| p.name.eq_ignore_ascii_case(&id.name)) {
                                                current_ty = p.ty.clone();
                                            } else {
                                                self.error(
                                                    DiagnosticCode::NoSuchField,
                                                    format!("no member '{}' on class '{}'", id.name, name),
                                                    id.range,
                                                );
                                                return Ty::Unknown;
                                            }
                                        }
                                    }
                                } else {
                                    return Ty::Unknown;
                                }
                            }
                            _ => {
                                self.error(
                                    DiagnosticCode::FieldAccessOnNonStruct,
                                    format!(
                                        "cannot access field '{}' on type '{}'",
                                        id.name,
                                        current_ty.display_name()
                                    ),
                                    id.range,
                                );
                                return Ty::Unknown;
                            }
                        }
                    }
                }
                AccessPart::Index(indices) => {
                    match current_ty.resolved() {
                        Ty::Array {
                            ranges,
                            element_type,
                        } => {
                            if indices.len() != ranges.len() {
                                self.error(
                                    DiagnosticCode::ArrayDimensionMismatch,
                                    format!(
                                        "array expects {} indices, got {}",
                                        ranges.len(),
                                        indices.len()
                                    ),
                                    va.range,
                                );
                            }
                            for idx in indices {
                                let idx_ty = self.check_expression(idx);
                                if !idx_ty.is_integer() && !matches!(idx_ty, Ty::Unknown) {
                                    self.error(
                                        DiagnosticCode::ArrayIndexTypeMismatch,
                                        format!(
                                            "array index must be integer, found '{}'",
                                            idx_ty.display_name()
                                        ),
                                        idx.range(),
                                    );
                                }
                            }
                            current_ty = *element_type.clone();
                        }
                        Ty::String { .. } => {
                            // String indexing returns a character (BYTE or WORD)
                            for idx in indices {
                                let idx_ty = self.check_expression(idx);
                                if !idx_ty.is_integer() && !matches!(idx_ty, Ty::Unknown) {
                                    self.error(
                                        DiagnosticCode::ArrayIndexTypeMismatch,
                                        format!(
                                            "string index must be integer, found '{}'",
                                            idx_ty.display_name()
                                        ),
                                        idx.range(),
                                    );
                                }
                            }
                            current_ty = Ty::Elementary(ElementaryType::Byte);
                        }
                        Ty::Unknown => {}
                        _ => {
                            self.error(
                                DiagnosticCode::IndexOnNonArray,
                                format!(
                                    "cannot index into type '{}'",
                                    current_ty.display_name()
                                ),
                                va.range,
                            );
                            return Ty::Unknown;
                        }
                    }
                }
                AccessPart::Deref => {
                    // Pointer dereference: ptr^ — the type becomes the pointed-to type
                    // For now, accept any type through deref (simplified)
                    current_ty = Ty::Unknown;
                }
                AccessPart::Partial(kind, _index) => {
                    // Partial access: .%X0 → BOOL, .%B0 → BYTE, .%W0 → WORD, .%D0 → DWORD
                    current_ty = match kind {
                        PartialAccessKind::Bit => Ty::Elementary(ElementaryType::Bool),
                        PartialAccessKind::Byte => Ty::Elementary(ElementaryType::Byte),
                        PartialAccessKind::Word => Ty::Elementary(ElementaryType::Word),
                        PartialAccessKind::DWord => Ty::Elementary(ElementaryType::Dword),
                        PartialAccessKind::LWord => Ty::Elementary(ElementaryType::Lword),
                    };
                }
            }
        }
        current_ty
    }

    /// Resolve a method by walking up the class hierarchy.
    /// Returns (defining_class_name, MethodDef) if found.
    fn resolve_class_method(&self, class_name: &str, method_name: &str) -> Option<(String, MethodDef)> {
        let mut current = Some(class_name.to_string());
        while let Some(ref cls_name) = current {
            if let Some(sym) = self.symbols.resolve_class(cls_name) {
                if let SymbolKind::Class { methods, base_class, .. } = &sym.kind {
                    if let Some(m) = methods.iter().find(|m| m.name.eq_ignore_ascii_case(method_name)) {
                        return Some((cls_name.clone(), m.clone()));
                    }
                    current = base_class.clone();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        None
    }

    fn check_function_call(&mut self, fc: &FunctionCallExpr) -> Ty {
        // Handle method calls: instance.Method(args)
        if fc.name.parts.len() == 2 {
            let obj_name = &fc.name.parts[0].name;
            let method_name = &fc.name.parts[1].name;
            if let Some((_sid, sym)) = self.symbols.resolve(self.current_scope, obj_name) {
                let sym_ty = sym.ty.clone();
                match sym_ty.resolved() {
                    Ty::Class { name: class_name } => {
                        self.symbols.mark_used(self.current_scope, obj_name);
                        let class_name = class_name.clone();
                        // Look up the method on the class (walk inheritance chain)
                        if let Some((_defining_class, method)) = self.resolve_class_method(&class_name, method_name) {
                            self.check_call_args(&fc.arguments, &method.params, &format!("{class_name}.{method_name}"), fc.range);
                            return method.return_type;
                        } else {
                            self.error(
                                DiagnosticCode::NoSuchField,
                                format!("no method '{method_name}' on class '{class_name}'"),
                                fc.name.parts[1].range,
                            );
                            return Ty::Unknown;
                        }
                    }
                    Ty::FunctionBlock { name: fb_name } => {
                        // FB method call: fb_instance.method() — currently FBs don't have methods,
                        // so fall through to normal resolution
                        let _ = fb_name;
                    }
                    _ => {}
                }
            }
        }

        let name = fc.name.as_str();
        // Clone the resolved symbol info to release the borrow on self.symbols
        let resolved = self
            .symbols
            .resolve(self.current_scope, &name)
            .map(|(sid, sym)| (sid, sym.kind.clone(), sym.ty.clone()));

        match resolved {
            Some((_sid, sym_kind, sym_ty)) => {
                self.symbols.mark_used(self.current_scope, &name);
                match sym_kind {
                    SymbolKind::Function { return_type, params } => {
                        self.check_call_args(&fc.arguments, &params, &name, fc.range);
                        return_type
                    }
                    SymbolKind::FunctionBlock { params, .. } => {
                        self.check_call_args(&fc.arguments, &params, &name, fc.range);
                        Ty::Void
                    }
                    SymbolKind::Class { params, is_abstract, .. } => {
                        if is_abstract {
                            self.error(
                                DiagnosticCode::CannotInstantiateAbstractClass,
                                format!("cannot instantiate abstract class '{name}'"),
                                fc.range,
                            );
                        }
                        self.check_call_args(&fc.arguments, &params, &name, fc.range);
                        Ty::Void
                    }
                    SymbolKind::Variable(_vk) => {
                        // Could be calling an FB or class instance
                        match sym_ty.resolved() {
                            Ty::FunctionBlock { name: fb_name } => {
                                let fb_name = fb_name.clone();
                                let params = self
                                    .symbols
                                    .resolve_pou(&fb_name)
                                    .and_then(|s| {
                                        if let SymbolKind::FunctionBlock { params, .. } = &s.kind {
                                            Some(params.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or_default();
                                self.check_call_args(&fc.arguments, &params, &fb_name, fc.range);
                                Ty::Void
                            }
                            Ty::Class { name: class_name } => {
                                let class_name = class_name.clone();
                                // Check if abstract
                                if let Some(sym) = self.symbols.resolve_class(&class_name) {
                                    if let SymbolKind::Class { is_abstract, .. } = &sym.kind {
                                        if *is_abstract {
                                            self.error(
                                                DiagnosticCode::CannotInstantiateAbstractClass,
                                                format!("cannot instantiate abstract class '{class_name}'"),
                                                fc.range,
                                            );
                                        }
                                    }
                                }
                                let params = self
                                    .symbols
                                    .resolve_class(&class_name)
                                    .and_then(|s| {
                                        if let SymbolKind::Class { params, .. } = &s.kind {
                                            Some(params.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or_default();
                                self.check_call_args(&fc.arguments, &params, &class_name, fc.range);
                                Ty::Void
                            }
                            _ => {
                                self.error(
                                    DiagnosticCode::NotCallable,
                                    format!("'{name}' is not callable"),
                                    fc.range,
                                );
                                Ty::Unknown
                            }
                        }
                    }
                    _ => {
                        self.error(
                            DiagnosticCode::NotCallable,
                            format!("'{name}' is not callable"),
                            fc.range,
                        );
                        Ty::Unknown
                    }
                }
            }
            None => {
                self.error(
                    DiagnosticCode::UndeclaredPou,
                    format!("undeclared function or function block '{name}'"),
                    fc.name.range,
                );
                Ty::Unknown
            }
        }
    }

    fn check_call_args(
        &mut self,
        args: &[Argument],
        params: &[ParamDef],
        callee_name: &str,
        call_range: TextRange,
    ) {
        let mut seen_params: Vec<String> = Vec::new();
        let mut positional_idx = 0;

        for arg in args {
            match arg {
                Argument::Named { name, value } => {
                    let arg_name_upper = name.name.to_uppercase();
                    if seen_params.contains(&arg_name_upper) {
                        self.error(
                            DiagnosticCode::DuplicateParam,
                            format!("duplicate parameter '{}' in call to '{}'", name.name, callee_name),
                            name.range,
                        );
                        continue;
                    }
                    seen_params.push(arg_name_upper.clone());

                    if let Some(param) = params
                        .iter()
                        .find(|p| p.name.eq_ignore_ascii_case(&name.name))
                    {
                        let val_ty = self.check_expression(value);
                        if !is_type_compatible(&val_ty, &param.ty)
                            && !matches!(val_ty, Ty::Unknown)
                            && !matches!(param.ty, Ty::Unknown)
                        {
                            self.error(
                                DiagnosticCode::ParamTypeMismatch,
                                format!(
                                    "parameter '{}' expects '{}', got '{}'",
                                    name.name,
                                    param.ty.display_name(),
                                    val_ty.display_name()
                                ),
                                value.range(),
                            );
                        }
                    } else {
                        self.error(
                            DiagnosticCode::UnknownParam,
                            format!(
                                "unknown parameter '{}' in call to '{}'",
                                name.name, callee_name
                            ),
                            name.range,
                        );
                    }
                }
                Argument::Positional(expr) => {
                    if positional_idx < params.len() {
                        let param = &params[positional_idx];
                        let val_ty = self.check_expression(expr);
                        if !is_type_compatible(&val_ty, &param.ty)
                            && !matches!(val_ty, Ty::Unknown)
                            && !matches!(param.ty, Ty::Unknown)
                        {
                            self.error(
                                DiagnosticCode::ParamTypeMismatch,
                                format!(
                                    "argument {} expects '{}', got '{}'",
                                    positional_idx + 1,
                                    param.ty.display_name(),
                                    val_ty.display_name()
                                ),
                                expr.range(),
                            );
                        }
                        positional_idx += 1;
                    } else {
                        self.error(
                            DiagnosticCode::TooManyPositionalArgs,
                            format!(
                                "too many arguments in call to '{}' (expected {})",
                                callee_name,
                                params.len()
                            ),
                            expr.range(),
                        );
                    }
                }
            }
        }

        // Check required parameters were provided
        for param in params {
            let param_upper = param.name.to_uppercase();
            let provided = seen_params.contains(&param_upper)
                || {
                    let idx = params
                        .iter()
                        .position(|p| p.name.eq_ignore_ascii_case(&param.name))
                        .unwrap();
                    idx < positional_idx
                };
            if !provided && matches!(param.var_kind, VarKind::VarInput | VarKind::VarInOut) {
                // Only warn for VAR_IN_OUT since VAR_INPUT has defaults
                if matches!(param.var_kind, VarKind::VarInOut) {
                    self.error(
                        DiagnosticCode::MissingRequiredParam,
                        format!(
                            "missing required VAR_IN_OUT parameter '{}' in call to '{}'",
                            param.name, callee_name
                        ),
                        call_range,
                    );
                }
            }
        }
    }

    fn check_unary(&mut self, u: &UnaryExpr) -> Ty {
        let operand_ty = self.check_expression(&u.operand);
        match u.op {
            UnaryOp::Neg => {
                if operand_ty.is_numeric() || matches!(operand_ty, Ty::Unknown) {
                    operand_ty
                } else {
                    self.error(
                        DiagnosticCode::IncompatibleUnaryOp,
                        format!(
                            "unary '-' requires a numeric type, found '{}'",
                            operand_ty.display_name()
                        ),
                        u.range,
                    );
                    Ty::Unknown
                }
            }
            UnaryOp::Not => {
                if operand_ty.is_bool()
                    || operand_ty.is_bit_string()
                    || matches!(operand_ty, Ty::Unknown)
                {
                    operand_ty
                } else {
                    self.error(
                        DiagnosticCode::IncompatibleUnaryOp,
                        format!(
                            "NOT requires BOOL or bit-string type, found '{}'",
                            operand_ty.display_name()
                        ),
                        u.range,
                    );
                    Ty::Unknown
                }
            }
        }
    }

    fn check_binary(&mut self, b: &BinaryExpr) -> Ty {
        let left_ty = self.check_expression(&b.left);
        let right_ty = self.check_expression(&b.right);

        if matches!(left_ty, Ty::Unknown) || matches!(right_ty, Ty::Unknown) {
            return Ty::Unknown;
        }

        match b.op {
            // Arithmetic: both must be numeric
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                // TIME + TIME, TIME - TIME are allowed
                if (left_ty.is_time() && right_ty.is_time())
                    && matches!(b.op, BinaryOp::Add | BinaryOp::Sub)
                {
                    return left_ty;
                }
                if !left_ty.is_numeric() {
                    self.error(
                        DiagnosticCode::IncompatibleBinaryOp,
                        format!(
                            "left operand of '{}' must be numeric, found '{}'",
                            binary_op_name(b.op),
                            left_ty.display_name()
                        ),
                        b.left.range(),
                    );
                    return Ty::Unknown;
                }
                if !right_ty.is_numeric() {
                    self.error(
                        DiagnosticCode::IncompatibleBinaryOp,
                        format!(
                            "right operand of '{}' must be numeric, found '{}'",
                            binary_op_name(b.op),
                            right_ty.display_name()
                        ),
                        b.right.range(),
                    );
                    return Ty::Unknown;
                }
                // MOD requires integers
                if matches!(b.op, BinaryOp::Mod)
                    && (!left_ty.is_integer() || !right_ty.is_integer())
                {
                    self.error(
                        DiagnosticCode::IncompatibleBinaryOp,
                        "MOD requires integer operands",
                        b.range,
                    );
                    return Ty::Unknown;
                }
                // Literal context typing: if one operand is a typed integer
                // variable and the other is a bare integer literal that fits
                // in that type, the result is the typed variable's type.
                // This makes `cycle : SINT := cycle + 1` work without an
                // explicit `SINT#1` annotation, matching Codesys/TwinCAT.
                if left_ty.is_integer() && literal_fits_in_target(&b.right, &left_ty) {
                    return left_ty;
                }
                if right_ty.is_integer() && literal_fits_in_target(&b.left, &right_ty) {
                    return right_ty;
                }
                common_type(&left_ty, &right_ty).unwrap_or(Ty::Unknown)
            }
            BinaryOp::Power => {
                if !left_ty.is_numeric() || !right_ty.is_numeric() {
                    self.error(
                        DiagnosticCode::IncompatibleBinaryOp,
                        "** requires numeric operands",
                        b.range,
                    );
                    return Ty::Unknown;
                }
                // Power always returns LREAL
                Ty::Elementary(ElementaryType::Lreal)
            }
            // Boolean: both must be BOOL (or bit-strings for bitwise)
            BinaryOp::And | BinaryOp::Or | BinaryOp::Xor => {
                if left_ty.is_bool() && right_ty.is_bool() {
                    Ty::Elementary(ElementaryType::Bool)
                } else if left_ty.is_bit_string() && right_ty.is_bit_string() {
                    common_type(&left_ty, &right_ty).unwrap_or(Ty::Unknown)
                } else {
                    self.error(
                        DiagnosticCode::IncompatibleBinaryOp,
                        format!(
                            "'{}' requires BOOL or matching bit-string operands, found '{}' and '{}'",
                            binary_op_name(b.op),
                            left_ty.display_name(),
                            right_ty.display_name()
                        ),
                        b.range,
                    );
                    Ty::Unknown
                }
            }
            // Comparison: both must be compatible, result is BOOL
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                if common_type(&left_ty, &right_ty).is_none() {
                    self.error(
                        DiagnosticCode::IncompatibleBinaryOp,
                        format!(
                            "cannot compare '{}' with '{}'",
                            left_ty.display_name(),
                            right_ty.display_name()
                        ),
                        b.range,
                    );
                }
                Ty::Elementary(ElementaryType::Bool)
            }
        }
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    fn expect_bool(&mut self, ty: &Ty, range: TextRange) {
        if !ty.is_bool() && !matches!(ty, Ty::Unknown) {
            self.error(
                DiagnosticCode::TypeMismatchCondition,
                format!(
                    "condition must be BOOL, found '{}'",
                    ty.display_name()
                ),
                range,
            );
        }
    }

    fn resolve_data_type(&self, dt: &DataType) -> Ty {
        match dt {
            DataType::Elementary(e) => Ty::Elementary(*e),
            DataType::Array(arr) => {
                let ranges: Vec<(i64, i64)> = arr
                    .ranges
                    .iter()
                    .map(|r| {
                        let lo = self.const_eval_int(&r.lower).unwrap_or(0);
                        let hi = self.const_eval_int(&r.upper).unwrap_or(0);
                        (lo, hi)
                    })
                    .collect();
                let element_type = self.resolve_data_type(&arr.element_type);
                Ty::Array {
                    ranges,
                    element_type: Box::new(element_type),
                }
            }
            DataType::String(s) => Ty::String {
                wide: s.wide,
                max_len: s.length.as_ref().and_then(|e| {
                    self.const_eval_int(e).map(|v| v as u32)
                }),
            },
            DataType::Ref(_) => Ty::Unknown, // REF_TO type — simplified for now
            DataType::UserDefined(qn) => {
                let name = qn.as_str();
                // Check if it's a known type
                if let Some(sym) = self.symbols.resolve_type(&name) {
                    sym.ty.clone()
                } else if let Some(sym) = self.symbols.resolve_pou(&name) {
                    // It's a function block type
                    sym.ty.clone()
                } else {
                    Ty::Unknown
                }
            }
        }
    }

    fn resolve_type_def_kind(&self, kind: &TypeDefKind, name: &str) -> Ty {
        match kind {
            TypeDefKind::Struct(s) => {
                let fields = s
                    .fields
                    .iter()
                    .map(|f| FieldDef {
                        name: f.name.name.clone(),
                        ty: self.resolve_data_type(&f.ty),
                    })
                    .collect();
                Ty::Struct {
                    name: name.to_string(),
                    fields,
                }
            }
            TypeDefKind::Enum(e) => Ty::Enum {
                name: name.to_string(),
                variants: e.values.iter().map(|v| v.name.name.clone()).collect(),
            },
            TypeDefKind::Subrange(s) => Ty::Subrange {
                name: name.to_string(),
                base: s.base_type,
                lower: self.const_eval_int(&s.lower).unwrap_or(0),
                upper: self.const_eval_int(&s.upper).unwrap_or(0),
            },
            TypeDefKind::Alias(dt) => {
                let target = self.resolve_data_type(dt);
                Ty::Alias {
                    name: name.to_string(),
                    target: Box::new(target),
                }
            }
        }
    }

    fn collect_params(&self, var_blocks: &[VarBlock]) -> Vec<ParamDef> {
        self.collect_params_by_kind(var_blocks, |k| {
            matches!(k, VarKind::VarInput | VarKind::VarInOut)
        })
    }

    fn collect_params_by_kind(
        &self,
        var_blocks: &[VarBlock],
        pred: impl Fn(VarKind) -> bool,
    ) -> Vec<ParamDef> {
        let mut params = Vec::new();
        for vb in var_blocks {
            if pred(vb.kind) {
                for decl in &vb.declarations {
                    let ty = self.resolve_data_type(&decl.ty);
                    for name in &decl.names {
                        params.push(ParamDef {
                            name: name.name.clone(),
                            ty: ty.clone(),
                            var_kind: vb.kind,
                        });
                    }
                }
            }
        }
        params
    }

    #[allow(clippy::only_used_in_recursion)]
    fn const_eval_int(&self, expr: &Expression) -> Option<i64> {
        match expr {
            Expression::Literal(lit) => match &lit.kind {
                LiteralKind::Integer(v) => Some(*v),
                _ => None,
            },
            Expression::Unary(u) if u.op == UnaryOp::Neg => {
                self.const_eval_int(&u.operand).map(|v| -v)
            }
            _ => None,
        }
    }

    // =========================================================================
    // Unused variable check (post-analysis)
    // =========================================================================

    fn check_unused(&mut self) {
        let mut warnings = Vec::new();
        let global_id = self.symbols.global_scope_id();
        for scope in self.symbols.scopes() {
            if scope.id == global_id {
                continue;
            }
            for sym in scope.symbols() {
                if let SymbolKind::Variable(vk) = &sym.kind {
                    if !sym.used && !sym.name.starts_with('_') {
                        let code = if matches!(vk, VarKind::VarInput | VarKind::VarOutput) {
                            DiagnosticCode::UnusedParameter
                        } else {
                            DiagnosticCode::UnusedVariable
                        };
                        warnings.push(Diagnostic::warning(
                            code,
                            format!("unused variable '{}'", sym.name),
                            sym.range,
                        ));
                    }
                    // Class/FB instances are never "assigned" via :=, but
                    // their state is managed implicitly via method calls.
                    let is_instance = matches!(
                        sym.ty,
                        Ty::FunctionBlock { .. } | Ty::Class { .. }
                    );
                    if !sym.assigned
                        && !is_instance
                        && !matches!(
                            vk,
                            VarKind::VarInput | VarKind::VarInOut | VarKind::VarExternal
                        )
                        && sym.used {
                            warnings.push(Diagnostic::warning(
                                DiagnosticCode::VariableNeverAssigned,
                                format!("variable '{}' is used but never assigned", sym.name),
                                sym.range,
                            ));
                        }
                }
            }
        }
        self.diagnostics.extend(warnings);
    }
}

// =============================================================================
// Native FB helpers
// =============================================================================

/// Map a [`st_comm_api::FieldDataType`] to the semantic [`Ty`].
fn field_data_type_to_semantic_ty(dt: st_comm_api::FieldDataType) -> Ty {
    use st_comm_api::FieldDataType;
    use st_syntax::ast::ElementaryType;
    Ty::Elementary(match dt {
        FieldDataType::Bool => ElementaryType::Bool,
        FieldDataType::Sint => ElementaryType::Sint,
        FieldDataType::Int => ElementaryType::Int,
        FieldDataType::Dint => ElementaryType::Dint,
        FieldDataType::Lint => ElementaryType::Lint,
        FieldDataType::Usint => ElementaryType::Usint,
        FieldDataType::Uint => ElementaryType::Uint,
        FieldDataType::Udint => ElementaryType::Udint,
        FieldDataType::Ulint => ElementaryType::Ulint,
        FieldDataType::Real => ElementaryType::Real,
        FieldDataType::Lreal => ElementaryType::Lreal,
        FieldDataType::Byte => ElementaryType::Byte,
        FieldDataType::Word => ElementaryType::Word,
        FieldDataType::Dword => ElementaryType::Dword,
        FieldDataType::Lword => ElementaryType::Lword,
        FieldDataType::String => return Ty::String { wide: false, max_len: None },
        FieldDataType::Time => ElementaryType::Time,
    })
}

// Type compatibility
// =============================================================================

/// Check if a value of type `from` can be used where type `to` is expected.
fn is_type_compatible(from: &Ty, to: &Ty) -> bool {
    let from = from.resolved();
    let to = to.resolved();
    if from == to {
        return true;
    }
    can_coerce(from, to)
}

/// If `expr` is an integer literal (possibly negated), return its value as `i128`.
/// Used to allow narrowing of literal values into smaller integer types when
/// the value fits — matches Codesys/TwinCAT behavior where `cycle : SINT := 0`
/// just works without an explicit `SINT#0` prefix.
fn integer_literal_value(expr: &Expression) -> Option<i128> {
    match expr {
        Expression::Literal(lit) => match &lit.kind {
            LiteralKind::Integer(n) => Some(*n as i128),
            LiteralKind::Typed { raw_value, .. } => raw_value.parse::<i128>().ok(),
            _ => None,
        },
        Expression::Parenthesized(inner) => integer_literal_value(inner),
        Expression::Unary(u) if matches!(u.op, UnaryOp::Neg) => {
            integer_literal_value(&u.operand).map(|v| -v)
        }
        _ => None,
    }
}

/// True if `expr` is an integer literal whose value fits in the target
/// integer type's range. Allows the semantic checker to accept narrowing
/// assignments like `cycle : SINT := 0` and `delay : USINT := 200` without
/// requiring the user to write `SINT#0` / `USINT#200` everywhere.
fn literal_fits_in_target(expr: &Expression, target: &Ty) -> bool {
    let target = target.resolved();
    let Ty::Elementary(elem) = target else {
        return false;
    };
    let Some((min, max)) = crate::types::integer_type_range(*elem) else {
        return false;
    };
    let Some(v) = integer_literal_value(expr) else {
        return false;
    };
    v >= min && v <= max
}

fn binary_op_name(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "MOD",
        BinaryOp::Power => "**",
        BinaryOp::And => "AND",
        BinaryOp::Or => "OR",
        BinaryOp::Xor => "XOR",
        BinaryOp::Eq => "=",
        BinaryOp::Ne => "<>",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::Le => "<=",
        BinaryOp::Ge => ">=",
    }
}
