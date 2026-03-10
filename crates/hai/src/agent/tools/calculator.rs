// use anyhow::Result;
// use rig::tool::ToolError;

// #[rig::tool_macro(
//     description = "Perform basic arithmetic operations",
//     params(operation = "The operation to perform (one of +, -, *, /)")
// )]
// fn calculator(x: i32, y: i32, operation: String) -> Result<i32, ToolError> {
//     match operation.as_str() {
//         "+" => Ok(x + y),
//         "-" => Ok(x - y),
//         "*" => Ok(x * y),
//         "/" => {
//             if y == 0 {
//                 Err(ToolError::ToolCallError("Division by zero".into()))
//             } else {
//                 Ok(x / y)
//             }
//         }
//         _ => Err(ToolError::ToolCallError(
//             format!("Unknown operation: {operation}").into(),
//         )),
//     }
// }

// #[rig::tool_macro]
// fn sum_numbers(numbers: Vec<i64>) -> Result<i64, ToolError> {
//     Ok(numbers.iter().sum())
// }
