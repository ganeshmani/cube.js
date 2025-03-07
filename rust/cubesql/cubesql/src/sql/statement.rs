use msql_srv::{Column, ColumnFlags, ColumnType};
use pg_srv::{BindValue, PgType};
use sqlparser::{
    ast,
    ast::{Expr, Value},
};

trait Visitor<'ast> {
    fn visit_value(&mut self, _val: &mut ast::Value) {}

    fn visit_identifier(&mut self, _identifier: &mut ast::Ident) {}

    fn visit_expr(&mut self, expr: &mut Expr) {
        match expr {
            Expr::Value(value) => self.visit_value(value),
            Expr::Identifier(identifier) => self.visit_identifier(identifier),
            Expr::CompoundIdentifier(identifiers) => {
                for ident in identifiers.iter_mut() {
                    self.visit_identifier(ident);
                }
            }
            Expr::Nested(v) => self.visit_expr(&mut *v),
            Expr::Cast { .. } => self.visit_cast(expr),
            Expr::Between {
                expr,
                negated: _,
                low,
                high,
            } => {
                self.visit_expr(&mut *expr);
                self.visit_expr(&mut *low);
                self.visit_expr(&mut *high);
            }
            Expr::BinaryOp { left, op: _, right } => {
                self.visit_expr(&mut *left);
                self.visit_expr(&mut *right);
            }
            Expr::InList { expr, list, .. } => {
                self.visit_expr(&mut *expr);

                for v in list.iter_mut() {
                    self.visit_expr(v);
                }
            }
            Expr::Case {
                operand,
                conditions,
                results,
                else_result,
            } => {
                if let Some(op) = operand {
                    self.visit_expr(&mut *op);
                }
                for con in conditions.iter_mut() {
                    self.visit_expr(&mut *con);
                }
                for res in results.iter_mut() {
                    self.visit_expr(&mut *res);
                }
                if let Some(res) = else_result {
                    self.visit_expr(&mut *res);
                }
            }
            Expr::IsNull(expr) | Expr::IsNotNull(expr) => self.visit_expr(expr),
            Expr::IsDistinctFrom(expr_1, expr_2) | Expr::IsNotDistinctFrom(expr_1, expr_2) => {
                self.visit_expr(expr_1);
                self.visit_expr(expr_2);
            }
            Expr::InSubquery { expr, subquery, .. } => {
                self.visit_expr(expr);
                self.visit_query(subquery);
            }
            Expr::InUnnest {
                expr, array_expr, ..
            } => {
                self.visit_expr(expr);
                self.visit_expr(array_expr);
            }
            Expr::UnaryOp { expr, .. } => {
                self.visit_expr(expr);
            }
            Expr::TryCast { expr, .. } | Expr::Extract { expr, .. } => self.visit_expr(expr),
            Expr::Substring {
                expr,
                substring_from,
                substring_for,
            } => {
                self.visit_expr(expr);
                if let Some(res) = substring_from {
                    self.visit_expr(res);
                }
                if let Some(res) = substring_for {
                    self.visit_expr(res);
                }
            }
            Expr::Trim { expr, trim_where } => {
                self.visit_expr(expr);
                if let Some((_, res)) = trim_where {
                    self.visit_expr(res);
                }
            }
            Expr::Collate { expr, collation } => {
                self.visit_expr(expr);
                for res in collation.0.iter_mut() {
                    self.visit_identifier(res);
                }
            }
            Expr::MapAccess { column, keys } => {
                self.visit_expr(column);
                for res in keys.iter_mut() {
                    self.visit_expr(res);
                }
            }
            Expr::Function(fun) => {
                for res in fun.name.0.iter_mut() {
                    self.visit_identifier(res);
                }
                self.visit_funtion_args(&mut fun.args);
                if let Some(over) = &mut fun.over {
                    for res in over.partition_by.iter_mut() {
                        self.visit_expr(res);
                    }
                    for order_expr in over.order_by.iter_mut() {
                        self.visit_expr(&mut order_expr.expr);
                    }
                }
            }
            Expr::Exists(query) | Expr::Subquery(query) => self.visit_query(query),
            Expr::ListAgg(list_agg) => {
                self.visit_expr(&mut list_agg.expr);
                if let Some(separator) = &mut list_agg.separator {
                    self.visit_expr(separator);
                }
                if let Some(on_overflow) = &mut list_agg.on_overflow {
                    if let ast::ListAggOnOverflow::Truncate { filler, .. } = on_overflow {
                        if let Some(expr) = filler {
                            self.visit_expr(expr);
                        }
                    }
                }
                for order_expr in list_agg.within_group.iter_mut() {
                    self.visit_expr(&mut order_expr.expr);
                }
            }
            Expr::GroupingSets(vec) | Expr::Cube(vec) | Expr::Rollup(vec) => {
                for v in vec.iter_mut() {
                    for expr in v.iter_mut() {
                        self.visit_expr(expr);
                    }
                }
            }
            Expr::Tuple(vec) => {
                for expr in vec.iter_mut() {
                    self.visit_expr(expr);
                }
            }
            Expr::ArrayIndex { obj, indexs } => {
                self.visit_expr(obj);
                for expr in indexs.iter_mut() {
                    self.visit_expr(expr);
                }
            }
            Expr::Array(arr) => {
                for expr in arr.elem.iter_mut() {
                    self.visit_expr(expr);
                }
            }
            Expr::DotExpr { expr, field } => {
                self.visit_expr(expr);
                self.visit_identifier(field);
            }
            Expr::TypedString { .. } => (),
        }
    }

    fn visit_table_factor(&mut self, factor: &mut ast::TableFactor) {
        match factor {
            ast::TableFactor::Derived {
                subquery, alias, ..
            } => {
                self.visit_query(subquery);
                self.visit_table_alias(alias);
            }
            ast::TableFactor::TableFunction { expr, alias } => {
                self.visit_expr(expr);
                self.visit_table_alias(alias);
            }
            ast::TableFactor::NestedJoin(table_with_joins) => {
                self.visit_table_with_joins(&mut *table_with_joins);
            }
            ast::TableFactor::Table {
                name,
                alias,
                args,
                with_hints,
            } => {
                for ident in name.0.iter_mut() {
                    self.visit_identifier(ident);
                }
                self.visit_table_alias(alias);
                self.visit_funtion_args(args);
                for hint in with_hints.iter_mut() {
                    self.visit_expr(hint);
                }
            }
        }
    }

    fn visit_join(&mut self, join: &mut ast::Join) {
        self.visit_table_factor(&mut join.relation);

        match &mut join.join_operator {
            ast::JoinOperator::Inner(constr)
            | ast::JoinOperator::LeftOuter(constr)
            | ast::JoinOperator::RightOuter(constr)
            | ast::JoinOperator::FullOuter(constr) => match constr {
                ast::JoinConstraint::On(expr) => {
                    self.visit_expr(expr);
                }
                ast::JoinConstraint::Using(idents) => {
                    for ident in idents.iter_mut() {
                        self.visit_identifier(ident);
                    }
                }
                ast::JoinConstraint::Natural | ast::JoinConstraint::None => (),
            },
            ast::JoinOperator::CrossJoin
            | ast::JoinOperator::CrossApply
            | ast::JoinOperator::OuterApply => (),
        }
    }

    fn visit_table_with_joins(&mut self, twj: &mut ast::TableWithJoins) {
        self.visit_table_factor(&mut twj.relation);

        for join in twj.joins.iter_mut() {
            self.visit_join(join);
        }
    }

    fn visit_select_item(&mut self, select: &mut ast::SelectItem) {
        match select {
            ast::SelectItem::ExprWithAlias { expr, .. } => self.visit_expr(expr),
            ast::SelectItem::UnnamedExpr(expr) => self.visit_expr(expr),
            ast::SelectItem::QualifiedWildcard(name) => {
                for ident in name.0.iter_mut() {
                    self.visit_identifier(ident);
                }
            }
            ast::SelectItem::Wildcard => (),
        }
    }

    fn visit_select(&mut self, select: &mut Box<ast::Select>) {
        if let Some(selection) = &mut select.selection {
            self.visit_expr(selection);
        };

        for projection in &mut select.projection {
            self.visit_select_item(projection);
        }

        for from in &mut select.from {
            self.visit_table_with_joins(from);
        }
    }

    fn visit_set_expr(&mut self, body: &mut ast::SetExpr) {
        match body {
            ast::SetExpr::Select(select) => self.visit_select(select),
            ast::SetExpr::Query(query) => self.visit_query(query),
            ast::SetExpr::SetOperation { left, right, .. } => {
                self.visit_set_expr(&mut *left);
                self.visit_set_expr(&mut *right);
            }
            ast::SetExpr::Values(vals) => {
                for v in vals.0.iter_mut() {
                    for expr in v.iter_mut() {
                        self.visit_expr(expr);
                    }
                }
            }
            ast::SetExpr::Insert(_) => (),
        }
    }

    fn visit_query(&mut self, query: &mut Box<ast::Query>) {
        self.visit_set_expr(&mut query.body);
    }

    fn visit_statement(&mut self, statement: &mut ast::Statement) {
        match statement {
            ast::Statement::Query(query) => self.visit_query(query),
            // TODO:
            _ => {}
        }
    }

    fn visit_cast(&mut self, expr: &mut Expr) {
        if let Expr::Cast { expr, .. } = expr {
            self.visit_expr(expr);
        }
    }

    fn visit_funtion_args(&mut self, args: &mut Vec<ast::FunctionArg>) {
        for a in args.iter_mut() {
            match a {
                ast::FunctionArg::Named { name, arg } => {
                    self.visit_identifier(name);
                    self.visit_function_arg_expr(arg);
                }
                ast::FunctionArg::Unnamed(arg) => self.visit_function_arg_expr(arg),
            }
        }
    }

    fn visit_function_arg_expr(&mut self, arg: &mut ast::FunctionArgExpr) {
        match arg {
            ast::FunctionArgExpr::Expr(expr) => self.visit_expr(expr),
            ast::FunctionArgExpr::QualifiedWildcard(name) => {
                for ident in name.0.iter_mut() {
                    self.visit_identifier(ident);
                }
            }
            ast::FunctionArgExpr::Wildcard => (),
        }
    }

    fn visit_table_alias(&mut self, alias: &mut Option<ast::TableAlias>) {
        if let Some(a) = alias {
            self.visit_identifier(&mut a.name);
            for ident in a.columns.iter_mut() {
                self.visit_identifier(ident);
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct FoundParameter {}

impl Into<Column> for FoundParameter {
    fn into(self) -> Column {
        Column {
            table: String::new(),
            column: "not implemented".to_owned(),
            coltype: ColumnType::MYSQL_TYPE_VAR_STRING,
            colflags: ColumnFlags::empty(),
        }
    }
}

#[derive(Debug)]
pub struct StatementParamsFinder {
    parameters: Vec<FoundParameter>,
}

impl StatementParamsFinder {
    pub fn new() -> Self {
        Self { parameters: vec![] }
    }

    pub fn find(mut self, stmt: &ast::Statement) -> Vec<FoundParameter> {
        self.visit_statement(&mut stmt.clone());

        self.parameters
    }
}

impl<'ast> Visitor<'ast> for StatementParamsFinder {
    fn visit_value(&mut self, v: &mut ast::Value) {
        match v {
            Value::Placeholder(_) => self.parameters.push(FoundParameter {}),
            _ => {}
        }
    }
}

#[derive(Debug)]
pub struct StatementParamsBinder {
    position: usize,
    values: Vec<BindValue>,
}

impl StatementParamsBinder {
    pub fn new(values: Vec<BindValue>) -> Self {
        Self {
            position: 0,
            values,
        }
    }

    pub fn bind(mut self, stmt: &mut ast::Statement) {
        self.visit_statement(stmt);
    }
}

impl<'ast> Visitor<'ast> for StatementParamsBinder {
    fn visit_value(&mut self, value: &mut ast::Value) {
        match &value {
            ast::Value::Placeholder(_) => {
                let to_replace = self.values.get(self.position).expect(
                    format!(
                        "Unable to find value for placeholder at position: {}",
                        self.position
                    )
                    .as_str(),
                );
                self.position += 1;

                match to_replace {
                    BindValue::String(v) => {
                        *value = ast::Value::SingleQuotedString(v.clone());
                    }
                    BindValue::Bool(v) => {
                        *value = ast::Value::Boolean(*v);
                    }
                    BindValue::UInt64(v) => {
                        *value = ast::Value::Number(v.to_string(), false);
                    }
                    BindValue::Int64(v) => {
                        *value = ast::Value::Number(v.to_string(), *v < 0_i64);
                    }
                    BindValue::Float64(v) => {
                        *value = ast::Value::Number(v.to_string(), *v < 0_f64);
                    }
                    BindValue::Null => {
                        *value = ast::Value::Null;
                    }
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
pub struct StatementPlaceholderReplacer {}

impl StatementPlaceholderReplacer {
    pub fn new() -> Self {
        Self {}
    }

    pub fn replace(mut self, stmt: &ast::Statement) -> ast::Statement {
        let mut result = stmt.clone();

        self.visit_statement(&mut result);

        result
    }
}

impl<'ast> Visitor<'ast> for StatementPlaceholderReplacer {
    fn visit_value(&mut self, value: &mut ast::Value) {
        match &value {
            ast::Value::Placeholder(_) => {
                *value = ast::Value::SingleQuotedString("replaced_placeholder".to_string());
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
pub struct CastReplacer {}

impl CastReplacer {
    pub fn new() -> Self {
        Self {}
    }

    pub fn replace(mut self, stmt: &ast::Statement) -> ast::Statement {
        let mut result = stmt.clone();

        self.visit_statement(&mut result);

        result
    }

    fn parse_value_to_str(&self, expr: &Value) -> Option<String> {
        match expr {
            Value::SingleQuotedString(str) | Value::DoubleQuotedString(str) => Some(str.clone()),
            _ => None,
        }
    }
}

impl<'ast> Visitor<'ast> for CastReplacer {
    fn visit_cast(&mut self, expr: &mut Expr) {
        if let Expr::Cast {
            expr: cast_expr,
            data_type,
        } = expr
        {
            match *data_type {
                ast::DataType::Regclass => match &**cast_expr {
                    Expr::Value(val) => {
                        let str_val = self.parse_value_to_str(&val);
                        if str_val.is_none() {
                            return;
                        }

                        let str_val = str_val.unwrap();
                        for typ in PgType::get_all() {
                            if typ.typname == str_val {
                                *expr = Expr::Value(Value::Number(typ.typrelid.to_string(), false));
                                break;
                            }
                        }
                    }
                    // TODO:
                    _ => (),
                },
                _ => self.visit_expr(&mut *cast_expr),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CubeError;
    use sqlparser::{dialect::PostgreSqlDialect, parser::Parser};

    fn test_binder(input: &str, output: &str, values: Vec<BindValue>) -> Result<(), CubeError> {
        let stmts = Parser::parse_sql(&PostgreSqlDialect {}, &input).unwrap();

        let binder = StatementParamsBinder::new(values);
        let mut input = stmts[0].clone();
        binder.bind(&mut input);

        assert_eq!(input.to_string(), output);

        Ok(())
    }

    #[test]
    fn test_binder_named() -> Result<(), CubeError> {
        test_binder(
            "SELECT ?",
            "SELECT 'test'",
            vec![BindValue::String("test".to_string())],
        )?;

        test_binder(
            "SELECT ? AS t1, ? AS t2",
            "SELECT 'test1' AS t1, NULL AS t2",
            vec![BindValue::String("test1".to_string()), BindValue::Null],
        )?;

        // binary op
        test_binder(
            r#"
                SELECT *
                FROM testdata
                WHERE fieldA = $1 AND fieldB = $2 OR (fieldC = $3 AND fieldD = $4)
            "#,
            "SELECT * FROM testdata WHERE fieldA = 'test' AND fieldB = 1 OR (fieldC = 2 AND fieldD = 2)",
            vec![
                BindValue::String("test".to_string()),
                BindValue::Int64(1),
                BindValue::UInt64(2),
                BindValue::Float64(2.0),
                BindValue::Bool(true),
            ],
        )?;

        // IN
        test_binder(
            r#"
                SELECT *
                FROM testdata
                WHERE fieldA IN ($1, $2)
            "#,
            "SELECT * FROM testdata WHERE fieldA IN ('test1', 'test2')",
            vec![
                BindValue::String("test1".to_string()),
                BindValue::String("test2".to_string()),
            ],
        )?;

        // BETWEEN
        test_binder(
            r#"
                SELECT *
                FROM testdata
                WHERE fieldA BETWEEN $1 AND $2
            "#,
            "SELECT * FROM testdata WHERE fieldA BETWEEN 'test1' AND 'test2'",
            vec![
                BindValue::String("test1".to_string()),
                BindValue::String("test2".to_string()),
            ],
        )?;

        test_binder(
            r#"
                SELECT *
                FROM testdata
                WHERE fieldA = $1
                UNION ALL
                SELECT *
                FROM testdata
                WHERE fieldA = $2
            "#,
            "SELECT * FROM testdata WHERE fieldA = 'test1' UNION ALL SELECT * FROM testdata WHERE fieldA = 'test2'",
            vec![
                BindValue::String(
                    "test1".to_string(),
                ),
                BindValue::String(
                    "test2".to_string(),
                ),
            ]
        )?;

        test_binder(
            r#"
                SELECT * FROM (
                    SELECT *
                    FROM testdata
                    WHERE fieldA = $1
                )
            "#,
            "SELECT * FROM (SELECT * FROM testdata WHERE fieldA = 'test1')",
            vec![BindValue::String("test1".to_string())],
        )?;

        Ok(())
    }

    fn assert_params_finder(input: &str, expected: Vec<FoundParameter>) -> Result<(), CubeError> {
        let stmts = Parser::parse_sql(&PostgreSqlDialect {}, &input).unwrap();

        let finder = StatementParamsFinder::new();
        let result = finder.find(&stmts[0]);

        assert_eq!(result, expected);

        Ok(())
    }

    #[test]
    fn test_placeholder_find() -> Result<(), CubeError> {
        assert_params_finder("SELECT $1", vec![FoundParameter {}])?;
        assert_params_finder("SELECT true as true_bool, false as false_bool", vec![])?;

        Ok(())
    }

    fn assert_placeholder_replacer(input: &str, output: &str) -> Result<(), CubeError> {
        let stmts = Parser::parse_sql(&PostgreSqlDialect {}, &input).unwrap();

        let binder = StatementPlaceholderReplacer::new();
        let result = binder.replace(&stmts[0]);

        assert_eq!(result.to_string(), output);

        Ok(())
    }

    #[test]
    fn test_placeholder_replacer() -> Result<(), CubeError> {
        assert_placeholder_replacer("SELECT ?", "SELECT 'replaced_placeholder'")?;

        Ok(())
    }
}
