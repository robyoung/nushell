use crate::data::base::coerce_compare;
use crate::data::base::shape::{Column, InlineShape};
use crate::data::primitive::style_primitive;
use chrono::DateTime;
use nu_errors::ShellError;
use nu_protocol::hir::Operator;
use nu_protocol::ShellTypeName;
use nu_protocol::{Primitive, Type, UntaggedValue};
use nu_source::{DebugDocBuilder, PrettyDebug, Span, Tagged};
use nu_table::TextStyle;
use num_traits::Zero;

pub fn date_from_str(s: Tagged<&str>) -> Result<UntaggedValue, ShellError> {
    let date = DateTime::parse_from_rfc3339(s.item).map_err(|err| {
        ShellError::labeled_error(
            &format!("Date parse error: {}", err),
            "original value",
            s.tag,
        )
    })?;

    let date = date.with_timezone(&chrono::offset::Utc);

    Ok(UntaggedValue::Primitive(Primitive::Date(date)))
}

pub fn merge_values(
    left: &UntaggedValue,
    right: &UntaggedValue,
) -> Result<UntaggedValue, (&'static str, &'static str)> {
    match (left, right) {
        (UntaggedValue::Row(columns), UntaggedValue::Row(columns_b)) => {
            Ok(UntaggedValue::Row(columns.merge_from(columns_b)))
        }
        (left, right) => Err((left.type_name(), right.type_name())),
    }
}

fn zero_division_error() -> UntaggedValue {
    UntaggedValue::Error(ShellError::untagged_runtime_error("division by zero"))
}

pub fn compute_values(
    operator: Operator,
    left: &UntaggedValue,
    right: &UntaggedValue,
) -> Result<UntaggedValue, (&'static str, &'static str)> {
    match (left, right) {
        (UntaggedValue::Primitive(lhs), UntaggedValue::Primitive(rhs)) => match (lhs, rhs) {
            (Primitive::Filesize(x), Primitive::Filesize(y)) => {
                let result = match operator {
                    Operator::Plus => Ok(x + y),
                    Operator::Minus => Ok(x - y),
                    _ => Err((left.type_name(), right.type_name())),
                }?;
                Ok(UntaggedValue::Primitive(Primitive::Filesize(result)))
            }
            (Primitive::Int(x), Primitive::Int(y)) => match operator {
                Operator::Plus => Ok(UntaggedValue::Primitive(Primitive::Int(x + y))),
                Operator::Minus => Ok(UntaggedValue::Primitive(Primitive::Int(x - y))),
                Operator::Multiply => Ok(UntaggedValue::Primitive(Primitive::Int(x * y))),
                Operator::Divide => {
                    if y.is_zero() {
                        Ok(zero_division_error())
                    } else if x - (y * (x / y)) == num_bigint::BigInt::from(0) {
                        Ok(UntaggedValue::Primitive(Primitive::Int(x / y)))
                    } else {
                        Ok(UntaggedValue::Primitive(Primitive::Decimal(
                            bigdecimal::BigDecimal::from(x.clone())
                                / bigdecimal::BigDecimal::from(y.clone()),
                        )))
                    }
                }
                _ => Err((left.type_name(), right.type_name())),
            },
            (Primitive::Decimal(x), Primitive::Int(y)) => {
                let result = match operator {
                    Operator::Plus => Ok(x + bigdecimal::BigDecimal::from(y.clone())),
                    Operator::Minus => Ok(x - bigdecimal::BigDecimal::from(y.clone())),
                    Operator::Multiply => Ok(x * bigdecimal::BigDecimal::from(y.clone())),
                    Operator::Divide => {
                        if y.is_zero() {
                            return Ok(zero_division_error());
                        }
                        Ok(x / bigdecimal::BigDecimal::from(y.clone()))
                    }
                    _ => Err((left.type_name(), right.type_name())),
                }?;
                Ok(UntaggedValue::Primitive(Primitive::Decimal(result)))
            }
            (Primitive::Int(x), Primitive::Decimal(y)) => {
                let result = match operator {
                    Operator::Plus => Ok(bigdecimal::BigDecimal::from(x.clone()) + y),
                    Operator::Minus => Ok(bigdecimal::BigDecimal::from(x.clone()) - y),
                    Operator::Multiply => Ok(bigdecimal::BigDecimal::from(x.clone()) * y),
                    Operator::Divide => {
                        if y.is_zero() {
                            return Ok(zero_division_error());
                        }
                        Ok(bigdecimal::BigDecimal::from(x.clone()) / y)
                    }
                    _ => Err((left.type_name(), right.type_name())),
                }?;
                Ok(UntaggedValue::Primitive(Primitive::Decimal(result)))
            }
            (Primitive::Decimal(x), Primitive::Decimal(y)) => {
                let result = match operator {
                    Operator::Plus => Ok(x + y),
                    Operator::Minus => Ok(x - y),
                    Operator::Multiply => Ok(x * y),
                    Operator::Divide => {
                        if y.is_zero() {
                            return Ok(zero_division_error());
                        }
                        Ok(x / y)
                    }
                    _ => Err((left.type_name(), right.type_name())),
                }?;
                Ok(UntaggedValue::Primitive(Primitive::Decimal(result)))
            }
            (Primitive::Date(x), Primitive::Date(y)) => match operator {
                Operator::Minus => Ok(UntaggedValue::Primitive(Primitive::from(
                    x.signed_duration_since(*y),
                ))),
                _ => Err((left.type_name(), right.type_name())),
            },
            (Primitive::Date(x), Primitive::Duration(_)) => {
                let result = match operator {
                    Operator::Plus => {
                        // FIXME: Not sure if I could do something better with the Span.
                        let y = Primitive::into_chrono_duration(rhs.clone(), Span::unknown())
                            .expect("Could not convert nushell Duration into chrono Duration.");
                        Ok(x.checked_add_signed(y).expect("Data overflow."))
                    }
                    _ => Err((left.type_name(), right.type_name())),
                }?;
                Ok(UntaggedValue::Primitive(Primitive::Date(result)))
            }
            (Primitive::Duration(x), Primitive::Duration(y)) => {
                let result = match operator {
                    Operator::Plus => Ok(x + y),
                    Operator::Minus => Ok(x - y),
                    _ => Err((left.type_name(), right.type_name())),
                }?;

                Ok(UntaggedValue::Primitive(Primitive::Duration(result)))
            }
            _ => Err((left.type_name(), right.type_name())),
        },
        _ => Err((left.type_name(), right.type_name())),
    }
}

/// If left is {{ Operator }} right
pub fn compare_values(
    operator: Operator,
    left: &UntaggedValue,
    right: &UntaggedValue,
) -> Result<bool, (&'static str, &'static str)> {
    let coerced = coerce_compare(left, right)?;
    let ordering = coerced.compare();

    use std::cmp::Ordering;

    let result = match (operator, ordering) {
        (Operator::Equal, Ordering::Equal) => true,
        (Operator::NotEqual, Ordering::Less) | (Operator::NotEqual, Ordering::Greater) => true,
        (Operator::LessThan, Ordering::Less) => true,
        (Operator::GreaterThan, Ordering::Greater) => true,
        (Operator::GreaterThanOrEqual, Ordering::Greater)
        | (Operator::GreaterThanOrEqual, Ordering::Equal) => true,
        (Operator::LessThanOrEqual, Ordering::Less)
        | (Operator::LessThanOrEqual, Ordering::Equal) => true,
        _ => false,
    };

    Ok(result)
}

pub fn format_type<'a>(value: impl Into<&'a UntaggedValue>, width: usize) -> String {
    Type::from_value(value.into()).colored_string(width)
}

pub fn format_leaf<'a>(value: impl Into<&'a UntaggedValue>) -> DebugDocBuilder {
    InlineShape::from_value(value.into()).format().pretty()
}

pub fn style_leaf<'a>(value: impl Into<&'a UntaggedValue>) -> TextStyle {
    match value.into() {
        UntaggedValue::Primitive(p) => style_primitive(p),
        _ => TextStyle::basic(),
    }
}

pub fn format_for_column<'a>(
    value: impl Into<&'a UntaggedValue>,
    column: impl Into<Column>,
) -> DebugDocBuilder {
    InlineShape::from_value(value.into())
        .format_for_column(column)
        .pretty()
}

#[cfg(test)]
mod tests {
    use super::UntaggedValue as v;
    use indexmap::indexmap;

    use super::merge_values;

    #[test]
    fn merges_tables() {
        let table_author_row = v::row(indexmap! {
            "name".into() => v::string("Andrés").into_untagged_value(),
            "country".into() => v::string("EC").into_untagged_value(),
            "date".into() => v::string("April 29-2020").into_untagged_value()
        });

        let other_table_author_row = v::row(indexmap! {
            "name".into() => v::string("YK").into_untagged_value(),
            "country".into() => v::string("US").into_untagged_value(),
            "date".into() => v::string("October 10-2019").into_untagged_value()
        });

        assert_eq!(
            other_table_author_row,
            merge_values(&table_author_row, &other_table_author_row).unwrap()
        );
    }
}
