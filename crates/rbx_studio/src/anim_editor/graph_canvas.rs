//! Geometry of the Animation Graph canvas, in canvas units (before pan and
//! zoom): where a node's box and pins sit, what the pointer is over, how an
//! imported graph with no saved positions is laid out, and the curve a wire
//! follows. Also the one-line text form a node property is edited in.

use rbx_animation::graph::{Graph, Node, NodeKind, Prop, Value};

pub(crate) const NODE_WIDTH: f32 = 184.0;
pub(crate) const HEADER: f32 = 28.0;
pub(crate) const PIN_ROW: f32 = 22.0;
/// How near (canvas units) a pin must be to the pointer to catch it.
pub(crate) const PIN_REACH: f32 = 9.0;
const COLUMN: f32 = 260.0;
const GAP: f32 = 28.0;

pub(crate) fn node_size(node: &Node) -> [f32; 2] {
    let rows = node.inputs.len().max(1) as f32;
    [NODE_WIDTH, HEADER + rows * PIN_ROW + 8.0]
}

pub(crate) fn input_pin(node: &Node, index: usize) -> [f32; 2] {
    [
        node.position.0,
        node.position.1 + HEADER + PIN_ROW * (index as f32 + 0.5),
    ]
}

/// Every node but the output has one output pin, beside its title.
pub(crate) fn output_pin(node: &Node) -> Option<[f32; 2]> {
    (node.kind != NodeKind::GraphOutput)
        .then_some([node.position.0 + NODE_WIDTH, node.position.1 + HEADER * 0.5])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hit {
    Output(usize),
    Input(usize, usize),
    Node(usize),
    Empty,
}

fn near(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= PIN_REACH
}

/// What lies under canvas point `p`. Pins win over bodies, since they
/// straddle a node's edge, and later nodes (drawn on top) over earlier ones.
pub(crate) fn hit(graph: &Graph, p: [f32; 2]) -> Hit {
    for (i, node) in graph.nodes.iter().enumerate().rev() {
        if output_pin(node).is_some_and(|o| near(o, p)) {
            return Hit::Output(i);
        }
        if let Some(k) = (0..node.inputs.len()).find(|&k| near(input_pin(node, k), p)) {
            return Hit::Input(i, k);
        }
    }
    for (i, node) in graph.nodes.iter().enumerate().rev() {
        let [w, h] = node_size(node);
        let (x, y) = node.position;
        if p[0] >= x && p[0] <= x + w && p[1] >= y && p[1] <= y + h {
            return Hit::Node(i);
        }
    }
    Hit::Empty
}

/// Lays a graph out in columns by distance from the output, right to left,
/// when none of its nodes has a position yet (a graph read from a file
/// written by something that keeps none).
pub(crate) fn arrange(graph: &mut Graph) -> bool {
    if graph.nodes.iter().any(|n| n.position != (0.0, 0.0)) || graph.nodes.len() < 2 {
        return false;
    }
    let mut depth = vec![usize::MAX; graph.nodes.len()];
    let Some(out) = graph.output() else {
        return false;
    };
    depth[out] = 0;
    let mut queue = vec![out];
    while let Some(at) = queue.pop() {
        let next: Vec<usize> = graph.nodes[at]
            .inputs
            .iter()
            .filter_map(|i| i.node)
            .collect();
        for child in next {
            if depth[child] == usize::MAX || depth[child] < depth[at] + 1 {
                depth[child] = depth[at] + 1;
                queue.push(child);
            }
        }
    }
    let deepest = depth
        .iter()
        .filter(|d| **d != usize::MAX)
        .max()
        .copied()
        .unwrap_or(0);
    let mut fill = vec![0.0f32; deepest + 2];
    for (i, node) in graph.nodes.iter_mut().enumerate() {
        let column = if depth[i] == usize::MAX {
            deepest + 1
        } else {
            depth[i]
        };
        let [_, h] = node_size(node);
        node.position = (-(column as f32) * COLUMN, fill[column]);
        fill[column] += h + GAP;
    }
    true
}

/// The box around every node, `[x, y, w, h]`, for fitting the view.
pub(crate) fn bounds(graph: &Graph) -> Option<[f32; 4]> {
    let mut iter = graph.nodes.iter();
    let first = iter.next()?;
    let [w, h] = node_size(first);
    let mut b = [
        first.position.0,
        first.position.1,
        first.position.0 + w,
        first.position.1 + h,
    ];
    for node in iter {
        let [w, h] = node_size(node);
        b[0] = b[0].min(node.position.0);
        b[1] = b[1].min(node.position.1);
        b[2] = b[2].max(node.position.0 + w);
        b[3] = b[3].max(node.position.1 + h);
    }
    Some([b[0], b[1], b[2] - b[0], b[3] - b[1]])
}

/// A wire from an output to an input: a horizontal S-curve, sampled.
pub(crate) fn wire(from: [f32; 2], to: [f32; 2]) -> Vec<[f32; 2]> {
    let reach = ((to[0] - from[0]).abs() * 0.5).max(40.0);
    let (c1, c2) = ([from[0] + reach, from[1]], [to[0] - reach, to[1]]);
    (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            let k = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            [0, 1].map(|a| k[0] * from[a] + k[1] * c1[a] + k[2] * c2[a] + k[3] * to[a])
        })
        .collect()
}

/// Reads a property as typed into its field: `$name` binds a parameter,
/// `=source` is an expression, a number or `true`/`false` is that value,
/// and anything else is text.
pub(crate) fn parse_prop(text: &str) -> Prop {
    let trimmed = text.trim();
    if let Some(name) = trimmed.strip_prefix('$').filter(|n| !n.is_empty()) {
        return Prop::Param(name.to_owned());
    }
    if let Some(source) = trimmed.strip_prefix('=') {
        return Prop::Expr(source.trim().to_owned());
    }
    match trimmed {
        "true" => Prop::Value(Value::Bool(true)),
        "false" => Prop::Value(Value::Bool(false)),
        // "0 (Infinity)" and "-1 (Random)" read as the number they show.
        _ => match trimmed
            .split_once(" (")
            .map_or(trimmed, |(n, _)| n)
            .parse::<f64>()
        {
            Ok(n) => Prop::Value(Value::Number(n)),
            Err(_) => Prop::Value(Value::Text(text.to_owned())),
        },
    }
}

/// [`format_prop`] for property `name`, with the editor's readings of its
/// special values: a LoopCount of 0 shows as "0 (Infinity)", a Seed of -1
/// as "-1 (Random)".
pub(crate) fn format_named_prop(name: &str, prop: &Prop) -> String {
    match prop {
        Prop::Value(Value::Number(n)) => {
            NodeKind::number_label(name, *n).map_or_else(|| format_prop(prop), str::to_owned)
        }
        _ => format_prop(prop),
    }
}

pub(crate) fn format_prop(prop: &Prop) -> String {
    match prop {
        Prop::Value(Value::Number(n)) => format!("{n}"),
        Prop::Value(Value::Bool(b)) => b.to_string(),
        Prop::Value(Value::Text(t)) => t.clone(),
        Prop::Param(name) => format!("${name}"),
        Prop::Expr(source) => format!("={source}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> (Graph, usize, usize) {
        let mut g = Graph::default();
        let clip = g.add(NodeKind::Clip);
        let speed = g.add(NodeKind::Speed);
        g.connect(clip, speed, "Pose");
        g.connect(speed, g.output().unwrap(), "Pose");
        (g, clip, speed)
    }

    #[test]
    fn arrange_puts_inputs_left_of_what_they_feed() {
        let (mut g, clip, speed) = chain();
        assert!(arrange(&mut g));
        let out = g.output().unwrap();
        assert!(g.nodes[speed].position.0 < g.nodes[out].position.0);
        assert!(g.nodes[clip].position.0 < g.nodes[speed].position.0);
        assert!(!arrange(&mut g), "positions now exist");
    }

    #[test]
    fn hits_pins_before_bodies() {
        let (mut g, clip, speed) = chain();
        arrange(&mut g);
        let o = output_pin(&g.nodes[clip]).unwrap();
        assert_eq!(hit(&g, o), Hit::Output(clip));
        let i = input_pin(&g.nodes[speed], 0);
        assert_eq!(hit(&g, [i[0] + 2.0, i[1]]), Hit::Input(speed, 0));
        let (x, y) = g.nodes[speed].position;
        assert_eq!(hit(&g, [x + 60.0, y + 10.0]), Hit::Node(speed));
        assert_eq!(hit(&g, [x - 500.0, y - 500.0]), Hit::Empty);
        assert!(output_pin(&g.nodes[g.output().unwrap()]).is_none());
    }

    #[test]
    fn bounds_and_wires() {
        let (mut g, _, _) = chain();
        arrange(&mut g);
        let [_, _, w, h] = bounds(&g).unwrap();
        assert!(w > NODE_WIDTH && h > 0.0);
        let curve = wire([0.0, 0.0], [100.0, 50.0]);
        assert_eq!(curve.first(), Some(&[0.0, 0.0]));
        assert_eq!(curve.last(), Some(&[100.0, 50.0]));
    }

    #[test]
    fn property_text_round_trips() {
        for text in ["$speed", "=speed * 2", "1.5", "true", "Loop"] {
            assert_eq!(format_prop(&parse_prop(text)), text);
        }
        for (name, text) in [("LoopCount", "0 (Infinity)"), ("Seed", "-1 (Random)")] {
            let prop = parse_prop(text);
            assert_eq!(format_named_prop(name, &prop), text);
        }
        assert_eq!(parse_prop("$"), Prop::Value(Value::Text("$".into())));
    }
}
