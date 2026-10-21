// The doc comments the bindings carry, rewritten from mruby's header
// dialect into the Markdown rustdoc renders.
//
// A build script is outside `cargo test`'s reach, so the rewrite lives
// here and both `build.rs` and the library's test build include it.

/// The tags mruby's headers write, each opening one list item. Any other
/// `@` word is prose, as Ruby's instance variables in an example are.
const COMMENT_TAGS: &[&str] = &["param", "return", "see", "note"];

/// One header comment, with its markers already stripped, as Markdown.
/// Tag lines become a list, and an indented example a fenced block
/// rustdoc neither reflows nor compiles. A `!!!c` line opening an
/// example names it C; any other example is plain text, since mruby's
/// mix Ruby and C in one block.
fn markdown_comment(comment: &str) -> String {
    let base = comment
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(indent_of)
        .min()
        .unwrap_or(0);
    let mut out: Vec<String> = Vec::new();
    let mut example: Vec<&str> = Vec::new();
    let mut in_tag = false;
    let mut after_colon = false;

    for line in comment.lines() {
        let text = line.trim();
        let depth = indent_of(line).saturating_sub(base);

        if text.is_empty() {
            if example.is_empty() {
                in_tag = false;
                out.push(String::new());
            } else {
                example.push("");
            }
            continue;
        }
        if text.starts_with("\\class ") || text.starts_with("@class ") {
            continue;
        }
        if let Some(item) = tag_item(text) {
            close_example(&mut out, &mut example);
            if !in_tag && out.last().is_some_and(|last| !last.is_empty()) {
                out.push(String::new());
            }
            out.push(item);
            in_tag = true;
            continue;
        }
        if in_tag && depth > 0 {
            if let Some(last) = out.last_mut() {
                last.push(' ');
                last.push_str(text);
            }
            continue;
        }
        let opens = depth >= 2 || (depth >= 1 && after_colon);
        if (!example.is_empty() && depth >= 1) || (example.is_empty() && opens) {
            example.push(line);
            continue;
        }
        close_example(&mut out, &mut example);
        in_tag = false;
        after_colon = text.ends_with(':');
        out.push(format!(" {text}"));
    }
    close_example(&mut out, &mut example);
    let start = out.iter().take_while(|line| line.is_empty()).count();
    out[start..].join("\n")
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The list item a tag line opens, or `None` for a line that is not one.
fn tag_item(text: &str) -> Option<String> {
    let rest = text.strip_prefix('@')?;
    let (tag, body) = rest.split_once(' ').unwrap_or((rest, ""));
    if !COMMENT_TAGS.contains(&tag) {
        return None;
    }
    let body = body.trim();
    let item = match tag {
        "param" => match body.split_once(' ') {
            Some((name, desc)) => format!("`{name}` {desc}"),
            None => format!("`{body}`"),
        },
        "return" => match body.strip_prefix('[').and_then(|b| b.split_once(']')) {
            Some((ty, desc)) => format!("`{ty}` {}", desc.trim()),
            None => body.to_owned(),
        },
        "see" => format!("`{body}`"),
        _ => body.to_owned(),
    };
    Some(format!(" - **{tag}** {item}").trim_end().to_owned())
}

/// Close the example being collected as one fenced block, its common
/// indent removed and its trailing blank lines left outside the fence.
/// The fence is longer than any backtick run the example holds, so no
/// line of it closes the block.
fn close_example(out: &mut Vec<String>, example: &mut Vec<&str>) {
    let mut trailing = 0;
    while example.last() == Some(&"") {
        example.pop();
        trailing += 1;
    }
    if example.is_empty() {
        out.extend(std::iter::repeat(String::new()).take(trailing));
        return;
    }
    let indent = example
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| indent_of(line))
        .min()
        .unwrap_or(0);
    let mut lines: Vec<&str> = example
        .iter()
        .map(|line| line.get(indent..).unwrap_or(""))
        .collect();
    let lang = if lines.first().map(|line| line.trim()) == Some("!!!c") {
        lines.remove(0);
        "c"
    } else {
        "text"
    };
    if out.last().is_some_and(|last| !last.is_empty()) {
        out.push(String::new());
    }
    let longest_run = lines
        .iter()
        .flat_map(|line| line.split(|c| c != '`'))
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest_run + 1).max(3));
    out.push(format!(" {fence}{lang}"));
    out.extend(
        lines
            .iter()
            .map(|line| format!(" {line}").trim_end().to_owned()),
    );
    out.push(format!(" {fence}"));
    out.extend(std::iter::repeat(String::new()).take(trailing));
    example.clear();
}

#[cfg(test)]
mod tests {
    use super::markdown_comment;

    #[test]
    fn tag_lines_become_a_list_after_the_prose() {
        let comment = " Defines a new module.\n\n @param mrb The current mruby state.\n @return [struct RClass *] Reference to the newly defined module.\n @see mrb_define_class";

        assert_eq!(
            markdown_comment(comment),
            " Defines a new module.\n\n - **param** `mrb` The current mruby state.\n - **return** `struct RClass *` Reference to the newly defined module.\n - **see** `mrb_define_class`"
        );
    }

    #[test]
    fn a_tag_continued_on_an_indented_line_stays_one_item() {
        let comment =
            " Get a nil mrb_value object.\n\n @return\n      nil mrb_value object reference.";

        assert_eq!(
            markdown_comment(comment),
            " Get a nil mrb_value object.\n\n - **return** nil mrb_value object reference."
        );
    }

    #[test]
    fn an_example_marked_c_is_fenced_as_c() {
        let comment = " Defines a new class.\n\n      !!!c\n      void init(mrb_state* mrb) {\n          define(mrb);\n      }\n\n @param mrb The current mruby state.";

        assert_eq!(
            markdown_comment(comment),
            " Defines a new class.\n\n ```c\n void init(mrb_state* mrb) {\n     define(mrb);\n }\n ```\n\n - **param** `mrb` The current mruby state."
        );
    }

    #[test]
    fn an_example_after_a_colon_is_fenced_at_one_column_of_indent() {
        let comment = " Prepends a module.\n\n Equivalent to:\n  module B\n    prepend A\n  end\n @param mrb The current mruby state.";

        assert_eq!(
            markdown_comment(comment),
            " Prepends a module.\n\n Equivalent to:\n\n ```text\n module B\n   prepend A\n end\n ```\n\n - **param** `mrb` The current mruby state."
        );
    }

    #[test]
    fn a_blank_line_inside_an_example_stays_inside_the_fence() {
        let comment = " Example:\n\n     a = 1\n\n     b = 2\n\n Done.";

        assert_eq!(
            markdown_comment(comment),
            " Example:\n\n ```text\n a = 1\n\n b = 2\n ```\n\n Done."
        );
    }

    #[test]
    fn an_example_holding_a_fence_is_fenced_past_it() {
        let comment = " Renders Markdown.\n\n      ```\n      code\n      ```\n\n @param mrb The current mruby state.";

        assert_eq!(
            markdown_comment(comment),
            " Renders Markdown.\n\n ````text\n ```\n code\n ```\n ````\n\n - **param** `mrb` The current mruby state."
        );
    }

    #[test]
    fn a_doxygen_class_line_is_dropped() {
        let comment = " \\class mrb_aspec\n\n Specifies the number of arguments.";

        assert_eq!(
            markdown_comment(comment),
            " Specifies the number of arguments."
        );
    }

    #[test]
    fn an_at_word_outside_the_tags_stays_prose() {
        let comment = " Reads @name from the receiver.";

        assert_eq!(markdown_comment(comment), " Reads @name from the receiver.");
    }
}
