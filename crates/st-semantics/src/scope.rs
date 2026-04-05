//! Hierarchical scope model for IEC 61131-3.
//!
//! Scopes form a tree: global → POU → (nested blocks if needed).
//! Each scope maps names (case-insensitive) to symbol definitions.

use crate::types::{FieldDef, Ty};
use st_syntax::ast::{TextRange, VarKind};
use std::collections::HashMap;

/// Unique identifier for a scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub usize);

/// A symbol definition.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub ty: Ty,
    pub kind: SymbolKind,
    pub range: TextRange,
    /// Whether this symbol has been read.
    pub used: bool,
    /// Whether this symbol has been written.
    pub assigned: bool,
}

/// Classification of symbols.
#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Variable(VarKind),
    Function { return_type: Ty, params: Vec<ParamDef> },
    FunctionBlock { params: Vec<ParamDef>, outputs: Vec<ParamDef> },
    Program { params: Vec<ParamDef> },
    Type,
}

/// Parameter definition for functions/FBs.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    pub name: String,
    pub ty: Ty,
    pub var_kind: VarKind,
}

/// A scope: a mapping of names → symbols.
#[derive(Debug)]
pub struct Scope {
    pub id: ScopeId,
    pub parent: Option<ScopeId>,
    pub name: String,
    symbols: HashMap<String, Symbol>,
}

impl Scope {
    pub fn new(id: ScopeId, parent: Option<ScopeId>, name: String) -> Self {
        Self {
            id,
            parent,
            name,
            symbols: HashMap::new(),
        }
    }

    /// Insert a symbol. Returns the previous symbol if the name was already defined.
    pub fn define(&mut self, symbol: Symbol) -> Option<Symbol> {
        let key = symbol.name.to_uppercase();
        self.symbols.insert(key, symbol)
    }

    /// Look up a symbol in this scope only (not parents).
    pub fn lookup_local(&self, name: &str) -> Option<&Symbol> {
        self.symbols.get(&name.to_uppercase())
    }

    /// Mutable lookup in this scope only.
    pub fn lookup_local_mut(&mut self, name: &str) -> Option<&mut Symbol> {
        self.symbols.get_mut(&name.to_uppercase())
    }

    /// Iterate over all symbols in this scope.
    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.values()
    }
}

/// The symbol table: a collection of scopes forming a tree.
#[derive(Debug)]
pub struct SymbolTable {
    scopes: Vec<Scope>,
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolTable {
    pub fn new() -> Self {
        let global = Scope::new(ScopeId(0), None, "global".to_string());
        Self {
            scopes: vec![global],
        }
    }

    pub fn global_scope_id(&self) -> ScopeId {
        ScopeId(0)
    }

    /// Create a new child scope. Returns its ID.
    pub fn create_scope(&mut self, parent: ScopeId, name: String) -> ScopeId {
        let id = ScopeId(self.scopes.len());
        self.scopes.push(Scope::new(id, Some(parent), name));
        id
    }

    pub fn scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.0]
    }

    pub fn scope_mut(&mut self, id: ScopeId) -> &mut Scope {
        &mut self.scopes[id.0]
    }

    /// Define a symbol in the given scope.
    pub fn define(&mut self, scope_id: ScopeId, symbol: Symbol) -> Option<Symbol> {
        self.scope_mut(scope_id).define(symbol)
    }

    /// Resolve a name by walking up the scope chain.
    pub fn resolve(&self, scope_id: ScopeId, name: &str) -> Option<(ScopeId, &Symbol)> {
        let mut current = Some(scope_id);
        while let Some(sid) = current {
            let scope = self.scope(sid);
            if let Some(sym) = scope.lookup_local(name) {
                return Some((sid, sym));
            }
            current = scope.parent;
        }
        None
    }

    /// Mark a symbol as used.
    pub fn mark_used(&mut self, scope_id: ScopeId, name: &str) {
        let mut current = Some(scope_id);
        while let Some(sid) = current {
            let scope = self.scope_mut(sid);
            if let Some(sym) = scope.lookup_local_mut(name) {
                sym.used = true;
                return;
            }
            current = self.scope(sid).parent;
        }
    }

    /// Mark a symbol as assigned.
    pub fn mark_assigned(&mut self, scope_id: ScopeId, name: &str) {
        let mut current = Some(scope_id);
        while let Some(sid) = current {
            let scope = self.scope_mut(sid);
            if let Some(sym) = scope.lookup_local_mut(name) {
                sym.assigned = true;
                return;
            }
            current = self.scope(sid).parent;
        }
    }

    /// Get all scopes.
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// Resolve a type name from the global scope.
    pub fn resolve_type(&self, name: &str) -> Option<&Symbol> {
        let global = self.scope(self.global_scope_id());
        let sym = global.lookup_local(name)?;
        if matches!(sym.kind, SymbolKind::Type) {
            Some(sym)
        } else {
            None
        }
    }

    /// Resolve a POU (function, FB, or program) from the global scope.
    pub fn resolve_pou(&self, name: &str) -> Option<&Symbol> {
        let global = self.scope(self.global_scope_id());
        let sym = global.lookup_local(name)?;
        match &sym.kind {
            SymbolKind::Function { .. }
            | SymbolKind::FunctionBlock { .. }
            | SymbolKind::Program { .. } => Some(sym),
            _ => None,
        }
    }

    /// Get the fields of a struct type by name.
    pub fn struct_fields(&self, name: &str) -> Option<&[FieldDef]> {
        let sym = self.resolve_type(name)?;
        if let Ty::Struct { fields, .. } = &sym.ty {
            Some(fields)
        } else {
            None
        }
    }

    /// Get the variants of an enum type by name.
    pub fn enum_variants(&self, name: &str) -> Option<&[String]> {
        let sym = self.resolve_type(name)?;
        if let Ty::Enum { variants, .. } = &sym.ty {
            Some(variants)
        } else {
            None
        }
    }
}
