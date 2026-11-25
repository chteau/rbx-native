//! The ScriptProfiler's Developer Console export read back into the dock's
//! callgraph and function views.

use super::ROWS;

/// One row of the ScriptProfiler view.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Line {
    pub(super) depth: usize,
    pub(super) name: String,
    /// `Source:Line`, empty for a frame task.
    pub(super) place: String,
    pub(super) ms: f64,
}

/// A ScriptProfiler session read back from its JSON.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Report {
    pub(super) callgraph: Vec<Line>,
    pub(super) functions: Vec<Line>,
    pub(super) total_ms: f64,
}

/// Reads the Developer Console's export (version 2) into the two views,
/// each sorted from most to least time, as Studio's are.
pub(super) fn report(json: &str) -> Option<Report> {
    let json: serde_json::Value = serde_json::from_str(json).ok()?;
    let nodes = json["Nodes"].as_array()?;
    let functions = json["Functions"].as_array()?;
    let ms = |v: &serde_json::Value| v["TotalDuration"].as_f64().unwrap_or(0.0) / 1000.0;
    let named = |id: usize| {
        let f = id
            .checked_sub(1)
            .and_then(|i| functions.get(i))
            .unwrap_or(&serde_json::Value::Null);
        let name = f["Name"].as_str().unwrap_or("<anonymous>").to_owned();
        let place = match (f["Source"].as_str(), f["Line"].as_u64()) {
            (Some(source), Some(line)) => format!("{source}:{line}"),
            (Some(source), None) => source.to_owned(),
            _ => String::new(),
        };
        (name, place)
    };
    fn walk(
        nodes: &[serde_json::Value],
        node: usize,
        depth: usize,
        out: &mut Vec<Line>,
        named: &dyn Fn(usize) -> (String, String),
    ) {
        let n = &nodes[node - 1];
        let mut children: Vec<(usize, usize)> = n["NodeIds"]
            .as_array()
            .into_iter()
            .flatten()
            .zip(n["FunctionIds"].as_array().into_iter().flatten())
            .filter_map(|(c, f)| Some((c.as_u64()? as usize, f.as_u64()? as usize)))
            .filter(|&(c, _)| c >= 1 && c <= nodes.len())
            .collect();
        let time = |c: usize| nodes[c - 1]["TotalDuration"].as_f64().unwrap_or(0.0);
        children.sort_by(|a, b| time(b.0).total_cmp(&time(a.0)));
        for (child, function) in children {
            if out.len() >= ROWS {
                return;
            }
            let (name, place) = named(function);
            out.push(Line {
                depth,
                name,
                place,
                ms: time(child) / 1000.0,
            });
            walk(nodes, child, depth + 1, out, named);
        }
    }
    let mut callgraph = Vec::new();
    let mut total_ms = 0.0;
    let mut categories: Vec<&serde_json::Value> = json["Categories"].as_array()?.iter().collect();
    let node_ms = |c: &serde_json::Value| {
        c["NodeId"]
            .as_u64()
            .and_then(|n| nodes.get((n as usize).checked_sub(1)?))
            .map_or(0.0, ms)
    };
    categories.sort_by(|a, b| node_ms(b).total_cmp(&node_ms(a)));
    for category in categories {
        let Some(node) = category["NodeId"].as_u64().map(|n| n as usize) else {
            continue;
        };
        if node == 0 || node > nodes.len() {
            continue;
        }
        let spent = ms(&nodes[node - 1]);
        total_ms += spent;
        callgraph.push(Line {
            depth: 0,
            name: category["Name"].as_str().unwrap_or("?").to_owned(),
            place: String::new(),
            ms: spent,
        });
        walk(nodes, node, 1, &mut callgraph, &named);
    }
    let mut flat: Vec<Line> = (1..=functions.len())
        .map(|id| {
            let (name, place) = named(id);
            Line {
                depth: 0,
                name,
                place,
                ms: ms(&functions[id - 1]),
            }
        })
        .collect();
    flat.sort_by(|a, b| b.ms.total_cmp(&a.ms));
    flat.truncate(ROWS);
    Some(Report {
        callgraph,
        functions: flat,
        total_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::report;

    #[test]
    fn the_export_reads_back_as_both_views() {
        let json = r#"{"Version":2,"SessionStartTime":1,"SessionEndTime":2,
            "Categories":[{"Name":"Heartbeat","NodeId":1}],
            "Nodes":[{"TotalDuration":3000,"FunctionIds":[1],"NodeIds":[2]},
                     {"TotalDuration":3000,"FunctionIds":[2],"NodeIds":[3]},
                     {"TotalDuration":1000,"FunctionIds":[],"NodeIds":[]}],
            "Functions":[{"Name":"tick","Source":"Workspace.Script","Line":3,"TotalDuration":3000,"Flags":0},
                         {"Source":"Workspace.Script","Line":9,"TotalDuration":1000,"Flags":0}]}"#;
        let report = report(json).expect("parses");
        assert_eq!(report.total_ms, 3.0);
        let names: Vec<(usize, &str, f64)> = report
            .callgraph
            .iter()
            .map(|l| (l.depth, l.name.as_str(), l.ms))
            .collect();
        assert_eq!(
            names,
            vec![
                (0, "Heartbeat", 3.0),
                (1, "tick", 3.0),
                (2, "<anonymous>", 1.0)
            ]
        );
        assert_eq!(report.functions[0].place, "Workspace.Script:3");
        assert_eq!(report.functions[1].name, "<anonymous>");
    }
}
