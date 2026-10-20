//! C 数组解析、Huffman trie 与生成代码。

use std::collections::BTreeMap;
use std::fmt::Write as _;

pub(crate) const TABLE_FILES: [&str; 2] = ["ts_103190_tables.c", "ts_103190_tables_part2.c"];
const EXPECTED_CODEBOOKS: usize = 84;
const EXPECTED_SYMBOLS: usize = 4_917;

/// 抽取形如 `const int NAME[N] = { ... };` 的一维整数数组。
///
/// C 文件里还有 `float` 与二维数组，本阶段只需要 Huffman 码本，故只收整数
/// 一维数组；其余留待需要时再纳入。
pub(crate) fn parse_arrays(text: &str, out: &mut BTreeMap<String, Vec<i64>>) {
    let text = strip_comments(text);
    let mut rest = text.as_str();

    while let Some(start) = rest.find("const ") {
        let after = &rest[start + "const ".len()..];
        let Some(brace) = after.find('{') else { break };
        let head = &after[..brace];
        rest = &after[brace..];

        let Some(open) = head.find('[') else { continue };
        let Some(close) = head.find(']') else {
            continue;
        };
        if close < open {
            continue;
        }
        // 只要一维：`]` 之后到 `=` 之间不应再有 `[`。
        if head[close..].contains('[') {
            continue;
        }
        let type_and_name = &head[..open];
        let mut words = type_and_name.split_whitespace();
        let (Some(kind), Some(name)) = (words.next(), words.next_back()) else {
            continue;
        };
        if !matches!(kind, "int" | "int32") {
            continue;
        }
        let declared: usize = head[open + 1..close].trim().parse().unwrap_or_else(|_| {
            panic!("{name} 的维度 {:?} 不是整数", &head[open + 1..close]);
        });

        let Some(end) = rest.find('}') else { break };
        let body = &rest[1..end];
        rest = &rest[end..];

        let values: Vec<i64> = body
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(|token| parse_int(token, name))
            .collect();
        assert_eq!(
            values.len(),
            declared,
            "{name} 声明 {declared} 项，实际 {} 项",
            values.len()
        );
        assert!(
            out.insert(name.to_owned(), values).is_none(),
            "{name} 重复定义"
        );
    }
}

/// 解析 C 表中的一维 `float` 数组。
///
/// 与 [`parse_arrays`] 分开而不是合并成一个泛型：整数表按 `0x` 前缀区分进制，
/// 浮点表则要求十进制字面量，两套记法混在一起会让「0x1p3 这种十六进制浮点被
/// 当成整数」这类错误变得难查。
pub(crate) fn parse_float_arrays(text: &str, out: &mut BTreeMap<String, Vec<f32>>) {
    let text = strip_comments(text);
    let mut rest = text.as_str();

    while let Some(start) = rest.find("const ") {
        let after = &rest[start + "const ".len()..];
        let Some(brace) = after.find('{') else { break };
        let head = &after[..brace];
        rest = &after[brace..];

        let Some(open) = head.find('[') else { continue };
        let Some(close) = head.find(']') else {
            continue;
        };
        if close < open || head[close..].contains('[') {
            continue;
        }
        let type_and_name = &head[..open];
        let mut words = type_and_name.split_whitespace();
        let (Some(kind), Some(name)) = (words.next(), words.next_back()) else {
            continue;
        };
        if kind != "float" {
            continue;
        }
        let declared: usize = head[open + 1..close].trim().parse().unwrap_or_else(|_| {
            panic!("{name} 的维度 {:?} 不是整数", &head[open + 1..close]);
        });

        let Some(end) = rest.find('}') else { break };
        let body = &rest[1..end];
        rest = &rest[end..];

        let values: Vec<f32> = body
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(|token| {
                assert!(
                    !token.contains('x') && !token.contains('X'),
                    "{name} 含非十进制浮点字面量 {token:?}"
                );
                // C 的 `float` 字面量常带 `f`/`F` 后缀；去掉后仍按 f32 就近舍入，
                // 与 C 编译器对 `const float` 的处理一致。
                let literal = token.trim_end_matches(['f', 'F']);
                let parsed: f32 = literal
                    .parse()
                    .unwrap_or_else(|_| panic!("{name} 含无法解析的值 {token:?}"));
                assert!(parsed.is_finite(), "{name} 含非有限值 {token:?}");
                parsed
            })
            .collect();
        assert_eq!(
            values.len(),
            declared,
            "{name} 声明 {declared} 项，实际 {} 项",
            values.len()
        );
        assert!(
            out.insert(name.to_owned(), values).is_none(),
            "{name} 重复定义"
        );
    }
}

/// 解析 C 表中的 `const float NAME[N][2]`，每行形如 `{ re, im }`。
///
/// [`parse_float_arrays`] 显式跳过二维声明（见那里的 `head[close..]` 判断），
/// 因此这里不是它的推广而是它的补集：把两个解析器合并会让「一维表被当成
/// 512×2 读」这类错位在计数相符时静默通过。
pub(crate) fn parse_complex_arrays(text: &str, out: &mut BTreeMap<String, Vec<[f32; 2]>>) {
    let text = strip_comments(text);
    let mut rest = text.as_str();

    while let Some(start) = rest.find("const float ") {
        let after = &rest[start + "const float ".len()..];
        let Some(brace) = after.find('{') else { break };
        let head = &after[..brace];
        rest = &after[brace..];

        // 只收「恰好两维、次维为 2」的声明；其余交给别的解析器。
        let dims: Vec<&str> = head
            .match_indices('[')
            .map(|(open, _)| {
                let tail = &head[open + 1..];
                &tail[..tail.find(']').unwrap_or(0)]
            })
            .collect();
        if dims.len() != 2 || dims[1].trim() != "2" {
            continue;
        }
        let Some(name) = head[..head.find('[').unwrap_or(0)]
            .split_whitespace()
            .next_back()
        else {
            continue;
        };
        let declared: usize = dims[0]
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{name} 的首维 {:?} 不是整数", dims[0]));

        // 外层大括号在 `rest[0]`，逐行的内层大括号在其后；找到整表的收尾。
        let Some(end) = rest.find("\n};") else {
            panic!("{name} 没有收尾的 `}};`");
        };
        let body = &rest[1..end];
        rest = &rest[end..];

        let mut values = Vec::with_capacity(declared);
        let mut cursor = body;
        while let Some(open) = cursor.find('{') {
            let tail = &cursor[open + 1..];
            let Some(close) = tail.find('}') else {
                panic!("{name} 有未闭合的行");
            };
            let row = &tail[..close];
            cursor = &tail[close + 1..];
            let mut parts = row.split(',').map(str::trim).filter(|t| !t.is_empty());
            let (Some(re), Some(im)) = (parts.next(), parts.next()) else {
                panic!("{name} 的一行不足两列：{row:?}");
            };
            assert!(parts.next().is_none(), "{name} 的一行超过两列：{row:?}");
            values.push([parse_float(name, re), parse_float(name, im)]);
        }
        assert_eq!(
            values.len(),
            declared,
            "{name} 声明 {declared} 行，实际 {} 行",
            values.len()
        );
        assert!(
            out.insert(name.to_owned(), values).is_none(),
            "{name} 重复定义"
        );
    }
}

/// C 的 `float` 字面量：去掉 `f`/`F` 后缀后按 f32 就近舍入。
fn parse_float(name: &str, token: &str) -> f32 {
    assert!(
        !token.contains('x') && !token.contains('X'),
        "{name} 含非十进制浮点字面量 {token:?}"
    );
    let parsed: f32 = token
        .trim_end_matches(['f', 'F'])
        .parse()
        .unwrap_or_else(|_| panic!("{name} 含无法解析的值 {token:?}"));
    assert!(parsed.is_finite(), "{name} 含非有限值 {token:?}");
    parsed
}

fn parse_int(token: &str, name: &str) -> i64 {
    let parsed = if let Some(hex) = token.strip_prefix("0x").or(token.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16)
    } else {
        token.parse::<i64>()
    };
    parsed.unwrap_or_else(|_| panic!("{name} 含无法解析的值 {token:?}"))
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let block = rest.find("/*");
        let line = rest.find("//");
        let (cut, terminator, skip) = match (block, line) {
            (None, None) => {
                out.push_str(rest);
                return out;
            }
            (Some(b), Some(l)) if b < l => (b, "*/", 2),
            (Some(b), None) => (b, "*/", 2),
            (_, Some(l)) => (l, "\n", 0),
        };
        out.push_str(&rest[..cut]);
        let after = &rest[cut..];
        match after.find(terminator) {
            Some(at) => {
                if skip == 0 {
                    out.push('\n');
                }
                rest = &after[at + terminator.len()..];
            }
            None => return out,
        }
    }
}

/// 一个内部节点的两条分支。非负为子节点下标，负数 `!v` 为符号下标。
type Node = [i16; 2];

pub(crate) fn emit(arrays: &BTreeMap<String, Vec<i64>>) -> String {
    let mut out = String::new();
    out.push_str(
        "// 由 build.rs 依据规范随附的 C 表生成，请勿手工编辑。\n\
         // 生成规则与校验条件见 crates/macindecode-ac4-decode/build.rs。\n\n",
    );

    // 不能只遍历完整配对：解析器漏掉任一侧时必须立即失败，而非静默少生成
    // 一张码本。两个方向都检查，以便错误直接指出孤立数组。
    for name in arrays.keys() {
        if let Some(stem) = name.strip_suffix("_CW") {
            assert!(
                arrays.contains_key(&format!("{stem}_LEN")),
                "{name} 缺少对应的 {stem}_LEN"
            );
        }
    }

    let mut names = Vec::new();
    for (name, lengths) in arrays {
        let Some(stem) = name.strip_suffix("_LEN") else {
            continue;
        };
        let codeword_name = format!("{stem}_CW");
        let codewords = arrays
            .get(&codeword_name)
            .unwrap_or_else(|| panic!("{name} 缺少对应的 {codeword_name}"));
        let nodes = build_trie(stem, lengths, codewords);
        let longest = lengths.iter().copied().max().unwrap_or(0);

        writeln!(
            out,
            "/// `{stem}`：{} 个符号，最长 {longest} 比特。",
            lengths.len()
        )
        .unwrap();
        writeln!(
            out,
            "pub static {stem}: HuffmanTable = HuffmanTable::new(&["
        )
        .unwrap();
        for chunk in nodes.chunks(8) {
            out.push_str("   ");
            for node in chunk {
                write!(out, " [{}, {}],", node[0], node[1]).unwrap();
            }
            out.push('\n');
        }
        out.push_str("]);\n\n");
        names.push((stem.to_owned(), lengths.clone(), codewords.clone()));
    }

    assert_eq!(
        names.len(),
        EXPECTED_CODEBOOKS,
        "当前规范基线应生成 {EXPECTED_CODEBOOKS} 张 Huffman 码本，实际生成 {} 张",
        names.len()
    );
    let symbols: usize = names.iter().map(|(_, lengths, _)| lengths.len()).sum();
    assert_eq!(
        symbols, EXPECTED_SYMBOLS,
        "当前规范基线应生成 {EXPECTED_SYMBOLS} 个符号，实际生成 {symbols} 个"
    );
    writeln!(out, "/// 本文件生成的码本张数。").unwrap();
    writeln!(
        out,
        "pub const GENERATED_CODEBOOKS: usize = {};\n",
        names.len()
    )
    .unwrap();

    emit_symbol_tables(&mut out, &names);
    out
}

/// 额外导出每张码本的原始 (码长, 码字)，只在测试构建中编译。
///
/// 用途是逐符号走一遍 trie，确认解码器与构造侧对比特序和叶子编码的理解
/// 一致。它不校验表值本身——表值由构建期的 Kraft 与前缀无关断言，以及
/// `spec/MANIFEST.json` 记录的成员哈希共同保证。
fn emit_symbol_tables(out: &mut String, books: &[(String, Vec<i64>, Vec<i64>)]) {
    out.push_str("#[cfg(test)]\n");
    out.push_str("pub static ALL_CODEBOOKS: &[(&str, &HuffmanTable, &[u8], &[u32])] = &[\n");
    for (stem, lengths, codewords) in books {
        write!(out, "    (\"{stem}\", &{stem}, &[").unwrap();
        for len in lengths {
            write!(out, "{len},").unwrap();
        }
        out.push_str("], &[");
        for codeword in codewords {
            write!(out, "{codeword},").unwrap();
        }
        out.push_str("]),\n");
    }
    out.push_str("];\n");
}

/// 构造解码用二叉 trie，并在构造过程中完成全部校验。
fn build_trie(stem: &str, lengths: &[i64], codewords: &[i64]) -> Vec<Node> {
    assert_eq!(
        lengths.len(),
        codewords.len(),
        "{stem}：_LEN 有 {} 项而 _CW 有 {} 项",
        lengths.len(),
        codewords.len()
    );

    // Kraft 等式。最长码字 29 比特，按 2^-32 定点累加不会溢出也无舍入。
    const SCALE_BITS: u32 = 32;
    let mut kraft: u64 = 0;
    for (index, &len) in lengths.iter().enumerate() {
        assert!(
            (1..=i64::from(SCALE_BITS)).contains(&len),
            "{stem}[{index}]：码长 {len} 越界"
        );
        kraft += 1u64 << (SCALE_BITS - u32::try_from(len).unwrap());
    }
    assert_eq!(
        kraft,
        1u64 << SCALE_BITS,
        "{stem}：Kraft 和不为 1，码本不是完备前缀码"
    );

    // 根节点先占位，分支填 0 表示尚未指向任何节点；构造完成后不应残留。
    let mut nodes: Vec<Node> = vec![[0, 0]];
    let mut assigned: Vec<[bool; 2]> = vec![[false, false]];

    for (symbol, (&len, &codeword)) in lengths.iter().zip(codewords).enumerate() {
        let len = u32::try_from(len).unwrap();
        assert!(
            codeword >= 0 && (codeword >> len) == 0,
            "{stem}[{symbol}]：码字 {codeword:#x} 超出 {len} 比特"
        );

        let mut node = 0usize;
        for depth in 0..len {
            let bit = usize::try_from((codeword >> (len - 1 - depth)) & 1).unwrap();
            let last = depth + 1 == len;
            if last {
                assert!(
                    !assigned[node][bit],
                    "{stem}[{symbol}]：码字 {codeword:#x} 与已有码字前缀冲突"
                );
                nodes[node][bit] = i16::try_from(!(symbol as i64)).unwrap();
                assigned[node][bit] = true;
            } else if assigned[node][bit] {
                let next = nodes[node][bit];
                assert!(
                    next >= 0,
                    "{stem}[{symbol}]：码字 {codeword:#x} 落在已有码字之后，非前缀码"
                );
                node = usize::try_from(next).unwrap();
            } else {
                nodes.push([0, 0]);
                assigned.push([false, false]);
                let child = i16::try_from(nodes.len() - 1).unwrap();
                nodes[node][bit] = child;
                assigned[node][bit] = true;
                node = usize::try_from(child).unwrap();
            }
        }
    }

    // 完备前缀码的 trie 是满二叉树：每个内部节点两条分支都已指派，且内部
    // 节点数恒为符号数减一。任一不成立说明码本有缺口。
    for (index, flags) in assigned.iter().enumerate() {
        assert!(
            flags[0] && flags[1],
            "{stem}：节点 {index} 存在未指派分支，码本有缺口"
        );
    }
    assert_eq!(
        nodes.len(),
        lengths.len() - 1,
        "{stem}：内部节点 {} 个，符号 {} 个，应恰好相差一",
        nodes.len(),
        lengths.len()
    );

    nodes
}
