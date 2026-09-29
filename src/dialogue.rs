pub struct DialogueSet {
    greetings: LinePool,
    idle: LinePool,
    clicked: LinePool,
}

struct LinePool {
    lines: Vec<String>,
    index: usize,
}

impl DialogueSet {
    pub fn load() -> Self {
        Self {
            greetings: LinePool::new(parse(include_str!("../assets/dialogues/greetings.txt"))),
            idle:      LinePool::new(parse(include_str!("../assets/dialogues/idle.txt"))),
            clicked:   LinePool::new(parse(include_str!("../assets/dialogues/clicked.txt"))),
        }
    }

    pub fn greeting(&mut self) -> String { self.greetings.next() }
    pub fn idle(&mut self) -> String     { self.idle.next() }
    pub fn clicked(&mut self) -> String  { self.clicked.next() }
}

impl LinePool {
    fn new(lines: Vec<String>) -> Self {
        let mut pool = Self {
            lines, index: 0,
        };
        fastrand::shuffle(&mut pool.lines);
        pool
    }

    fn next(&mut self) -> String {
        if self.lines.is_empty() {
            return "...".to_string();
        }
        let line = self.lines[self.index].clone();

        self.index += 1;
        if self.index >= self.lines.len() {
            fastrand::shuffle(&mut self.lines);
            self.index = 0;
        }

        line
    }
}

fn parse(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}