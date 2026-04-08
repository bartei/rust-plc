//! Typed AST nodes for IEC 61131-3 Structured Text.
//!
//! Every node carries a [`TextRange`] for source location mapping.

use serde::{Deserialize, Serialize};

/// A byte-offset range in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

impl TextRange {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// A row/column position in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    pub row: usize,
    pub column: usize,
}

/// A source file: the top-level compilation unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub items: Vec<TopLevelItem>,
    pub range: TextRange,
}

/// A top-level declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TopLevelItem {
    Program(ProgramDecl),
    Function(FunctionDecl),
    FunctionBlock(FunctionBlockDecl),
    Class(ClassDecl),
    Interface(InterfaceDecl),
    TypeDeclaration(TypeDeclarationBlock),
    GlobalVarDeclaration(VarBlock),
}

// =============================================================================
// Program Organization Units
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramDecl {
    pub name: Identifier,
    pub var_blocks: Vec<VarBlock>,
    pub body: Vec<Statement>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDecl {
    pub name: Identifier,
    pub return_type: DataType,
    pub var_blocks: Vec<VarBlock>,
    pub body: Vec<Statement>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionBlockDecl {
    pub name: Identifier,
    pub var_blocks: Vec<VarBlock>,
    pub body: Vec<Statement>,
    pub range: TextRange,
}

// =============================================================================
// OOP extensions (IEC 61131-3 Ed.3)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassDecl {
    pub name: Identifier,
    pub base_class: Option<String>,
    pub interfaces: Vec<String>,
    pub is_abstract: bool,
    pub is_final: bool,
    pub var_blocks: Vec<VarBlock>,
    pub methods: Vec<MethodDecl>,
    pub properties: Vec<PropertyDecl>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodDecl {
    pub access: AccessSpecifier,
    pub name: Identifier,
    pub return_type: Option<DataType>,
    pub var_blocks: Vec<VarBlock>,
    pub body: Vec<Statement>,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_override: bool,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceDecl {
    pub name: Identifier,
    pub base_interfaces: Vec<String>,
    pub methods: Vec<MethodPrototype>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodPrototype {
    pub name: Identifier,
    pub return_type: Option<DataType>,
    pub var_blocks: Vec<VarBlock>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyDecl {
    pub access: AccessSpecifier,
    pub name: Identifier,
    pub ty: DataType,
    pub get_body: Option<PropertyAccessor>,
    pub set_body: Option<PropertyAccessor>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyAccessor {
    pub var_blocks: Vec<VarBlock>,
    pub body: Vec<Statement>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessSpecifier {
    Public,
    Private,
    Protected,
    Internal,
}

// =============================================================================
// Type declarations
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDeclarationBlock {
    pub definitions: Vec<TypeDefinition>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDefinition {
    pub name: Identifier,
    pub ty: TypeDefKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeDefKind {
    Struct(StructType),
    Enum(EnumType),
    Subrange(SubrangeType),
    Alias(DataType),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructType {
    pub fields: Vec<StructField>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructField {
    pub name: Identifier,
    pub ty: DataType,
    pub default: Option<Expression>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumType {
    pub values: Vec<EnumValue>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumValue {
    pub name: Identifier,
    pub value: Option<Literal>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubrangeType {
    pub base_type: ElementaryType,
    pub lower: Expression,
    pub upper: Expression,
    pub range: TextRange,
}

// =============================================================================
// Variable declarations
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarBlock {
    pub kind: VarKind,
    pub qualifiers: Vec<VarQualifier>,
    pub declarations: Vec<VarDeclaration>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VarKind {
    Var,
    VarInput,
    VarOutput,
    VarInOut,
    VarGlobal,
    VarExternal,
    VarTemp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VarQualifier {
    Retain,
    Persistent,
    Constant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarDeclaration {
    pub names: Vec<Identifier>,
    pub ty: DataType,
    pub initial_value: Option<Expression>,
    pub range: TextRange,
}

// =============================================================================
// Data types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataType {
    Elementary(ElementaryType),
    Array(Box<ArrayType>),
    String(StringType),
    Ref(Box<DataType>),
    UserDefined(QualifiedName),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementaryType {
    Bool,
    Sint,
    Int,
    Dint,
    Lint,
    Usint,
    Uint,
    Udint,
    Ulint,
    Real,
    Lreal,
    Byte,
    Word,
    Dword,
    Lword,
    Time,
    Ltime,
    Date,
    Ldate,
    Tod,
    Ltod,
    Dt,
    Ldt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArrayType {
    pub ranges: Vec<ArrayRange>,
    pub element_type: DataType,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArrayRange {
    pub lower: Expression,
    pub upper: Expression,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StringType {
    pub wide: bool,
    pub length: Option<Expression>,
    pub range: TextRange,
}

// =============================================================================
// Statements
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    Assignment(AssignmentStmt),
    FunctionCall(FunctionCallExpr),
    If(IfStmt),
    Case(CaseStmt),
    For(ForStmt),
    While(WhileStmt),
    Repeat(RepeatStmt),
    Return(TextRange),
    Exit(TextRange),
    Empty(TextRange),
}

impl Statement {
    pub fn range(&self) -> TextRange {
        match self {
            Self::Assignment(s) => s.range,
            Self::FunctionCall(s) => s.range,
            Self::If(s) => s.range,
            Self::Case(s) => s.range,
            Self::For(s) => s.range,
            Self::While(s) => s.range,
            Self::Repeat(s) => s.range,
            Self::Return(r) | Self::Exit(r) | Self::Empty(r) => *r,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentStmt {
    pub target: VariableAccess,
    pub value: Expression,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IfStmt {
    pub condition: Expression,
    pub then_body: Vec<Statement>,
    pub elsif_clauses: Vec<ElsifClause>,
    pub else_body: Option<Vec<Statement>>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElsifClause {
    pub condition: Expression,
    pub body: Vec<Statement>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseStmt {
    pub expression: Expression,
    pub branches: Vec<CaseBranch>,
    pub else_body: Option<Vec<Statement>>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseBranch {
    pub selectors: Vec<CaseSelector>,
    pub body: Vec<Statement>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CaseSelector {
    Single(Expression),
    Range(Expression, Expression),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForStmt {
    pub variable: Identifier,
    pub from: Expression,
    pub to: Expression,
    pub by: Option<Expression>,
    pub body: Vec<Statement>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhileStmt {
    pub condition: Expression,
    pub body: Vec<Statement>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatStmt {
    pub body: Vec<Statement>,
    pub condition: Expression,
    pub range: TextRange,
}

// =============================================================================
// Expressions
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expression {
    Literal(Literal),
    Variable(VariableAccess),
    FunctionCall(Box<FunctionCallExpr>),
    Unary(Box<UnaryExpr>),
    Binary(Box<BinaryExpr>),
    Parenthesized(Box<Expression>),
    This(TextRange),
    Super(TextRange),
}

impl Expression {
    pub fn range(&self) -> TextRange {
        match self {
            Self::Literal(l) => l.range,
            Self::Variable(v) => v.range,
            Self::FunctionCall(f) => f.range,
            Self::Unary(u) => u.range,
            Self::Binary(b) => b.range,
            Self::Parenthesized(e) => e.range(),
            Self::This(r) | Self::Super(r) => *r,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub operand: Expression,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryExpr {
    pub op: BinaryOp,
    pub left: Expression,
    pub right: Expression,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Power,
    And,
    Or,
    Xor,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallExpr {
    pub name: QualifiedName,
    pub arguments: Vec<Argument>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Argument {
    Positional(Expression),
    Named { name: Identifier, value: Expression },
}

// =============================================================================
// Variable access
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableAccess {
    pub parts: Vec<AccessPart>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessPart {
    Identifier(Identifier),
    Index(Vec<Expression>),
    Deref,
    /// Partial bit/byte/word/dword access: .%X0, .%B1, .%W0, .%D0
    Partial(PartialAccessKind, u32),
}

/// The size of a partial access operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartialAccessKind {
    /// Bit access (.%X0 .. .%X63) → result is BOOL
    Bit,
    /// Byte access (.%B0 .. .%B7) → result is BYTE
    Byte,
    /// Word access (.%W0 .. .%W3) → result is WORD
    Word,
    /// DWord access (.%D0 .. .%D1) → result is DWORD
    DWord,
    /// LWord access (.%L0) → result is LWORD
    LWord,
}

// =============================================================================
// Literals
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Literal {
    pub kind: LiteralKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LiteralKind {
    Integer(i64),
    Real(f64),
    String(String),
    Bool(bool),
    /// Raw text for TIME, DATE, TOD, DT literals — parsed later by the semantic layer.
    Time(String),
    Date(String),
    Tod(String),
    Dt(String),
    /// Null pointer literal
    Null,
    /// Typed literal, e.g. `INT#5`
    Typed {
        ty: ElementaryType,
        raw_value: String,
    },
}

// =============================================================================
// Common
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identifier {
    pub name: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualifiedName {
    pub parts: Vec<Identifier>,
    pub range: TextRange,
}

impl QualifiedName {
    pub fn as_str(&self) -> String {
        self.parts
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}
