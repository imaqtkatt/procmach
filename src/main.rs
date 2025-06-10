use core::panic;
use std::{
  cell::{Cell, UnsafeCell},
  collections::{BTreeMap, VecDeque},
  rc::Rc,
  sync::atomic::AtomicUsize,
};

mod tests;

static PID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Pid(usize);

impl std::fmt::Debug for Pid {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "PID#[{:#06x}]", self.0)
  }
}

impl Pid {
  pub fn next() -> Self {
    Self(PID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Atom(u32);

const ATOM_EXIT: Atom = Atom(0);
const ATOM_NORMAL: Atom = Atom(1);

#[derive(Clone, Copy, Debug)]
pub struct HeapHandle {
  index: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum Value {
  Pid(Pid),
  Int(i32),
  Atom(Atom),
  Heap(HeapHandle),
}

#[derive(Clone, Debug)]
pub enum HeapValue {
  Str(String),
  Tuple(Vec<Value>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcessState {
  // TODO: do we need this Runnable state?
  Runnable,
  Waiting,
  Running,
  Exiting,
}

#[derive(Clone, Copy, Debug)]
pub enum Instruction {
  DbgStack,
  DbgDrop,
  PushInt(i32),
  PushPid(Pid),
  MakeTuple(u16),
  GetTuple(u16),
  Add,
  Sub,
  Mul,
  Div,
  This,
  Link,
  Monitor,
  Send,
  Receive,
  Store(u16),
  Load(u16),
  Goto(u16),
  Return,
  ExitReason,
  Exit,
}

#[derive(Clone, Debug)]
pub struct Frame {
  pub ip: Cell<usize>,
  pub code: Vec<Instruction>,
  pub locals: Vec<Value>,
}

#[derive(Debug, Default)]
#[repr(transparent)]
pub struct Mailbox {
  queue: VecDeque<Value>,
}

#[derive(Debug, Default)]
pub struct Heap {
  memory: Vec<HeapValue>,
}

impl Heap {
  fn get(&self, handle: HeapHandle) -> &HeapValue {
    &self.memory[handle.index]
  }

  fn allocate_string(&mut self, s: impl Into<String>) -> Value {
    let index = self.memory.len();
    self.memory.push(HeapValue::Str(s.into()));
    Value::Heap(HeapHandle { index })
  }

  fn allocate_tuple(&mut self, values: Vec<Value>) -> Value {
    let index = self.memory.len();
    self.memory.push(HeapValue::Tuple(values));
    Value::Heap(HeapHandle { index })
  }

  fn deep_clone_at(&self, value: Value, other: &mut Heap) -> Value {
    fn deep_clone_value(this_heap: &Heap, value: Value, heap: &mut Heap) -> Value {
      match value {
        Value::Pid(pid) => Value::Pid(pid),
        Value::Int(int) => Value::Int(int),
        Value::Atom(atom) => Value::Atom(atom),
        Value::Heap(heap_handle) => match &this_heap.memory[heap_handle.index] {
          HeapValue::Str(str) => heap.allocate_string(str),
          HeapValue::Tuple(values) => {
            let mut new_values = vec![];
            for value in values.iter().cloned() {
              new_values.push(deep_clone_value(this_heap, value, heap));
            }
            heap.allocate_tuple(new_values)
          }
        },
      }
    }

    deep_clone_value(self, value, other)
  }
}

#[derive(Debug)]
pub struct Process {
  pub pid: Pid,
  pub ip: Cell<usize>,
  pub state: ProcessState,
  pub stack: Vec<Value>,
  pub frames: Vec<Frame>,
  pub mailbox: VecDeque<Value>,
  pub scheduler: Rc<UnsafeCell<Scheduler>>,
  pub links: Vec<Pid>,
  pub monitors: Vec<Pid>,
  pub heap: Heap,
  /// if None, exit normally
  pub exit_reason: Option<Value>,
}

impl Process {
  pub fn fetch(&mut self) -> Instruction {
    let frame = self.frames.last_mut().expect("not empty frames");
    let ip = self.ip.get_mut();
    let instruction = frame.code[*ip];
    *ip += 1;
    instruction
  }

  fn rewind(&mut self) {
    let ip = self.ip.get_mut();
    debug_assert_ne!(*ip, 0);
    *ip -= 1;
  }

  pub fn step(&mut self) -> ProcessState {
    macro_rules! pop_or_exit {
      ($self:expr, $name:ident) => {
        let Some($name) = $self.stack.pop() else {
          $self.state = ProcessState::Exiting;
          return $self.state;
        };
      };
    }

    let instruction = self.fetch();
    println!("{:?} : step {instruction:?}", self.pid);
    match instruction {
      Instruction::DbgStack => {
        println!("{:?}", self.stack);
      }
      Instruction::DbgDrop => {
        pop_or_exit!(self, value);
        fn debug_value(
          value: &Value,
          heap: &Heap,
          f: &mut std::fmt::Formatter,
        ) -> std::fmt::Result {
          match value {
            Value::Pid(pid) => write!(f, "Pid({:?})", pid),
            Value::Int(int) => write!(f, "Int({:?})", int),
            Value::Atom(atom) => write!(f, "Atom({:?})", atom),
            Value::Heap(heap_handle) => match heap.get(*heap_handle) {
              HeapValue::Str(str) => write!(f, "{str}"),
              HeapValue::Tuple(values) => {
                write!(f, "{{")?;
                for (i, value) in values.iter().enumerate() {
                  if i != 0 {
                    write!(f, ", ")?;
                  }
                  debug_value(value, heap, f)?;
                }
                write!(f, "}}")
              }
            },
          }
        }
        struct Wrapper<'a>(&'a Value, &'a Heap);
        impl std::fmt::Debug for Wrapper<'_> {
          fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            debug_value(self.0, self.1, f)
          }
        }
        println!("{:?}", Wrapper(&value, &self.heap))
      }
      Instruction::PushInt(i) => self.stack.push(Value::Int(i)),
      Instruction::PushPid(pid) => self.stack.push(Value::Pid(pid)),
      Instruction::MakeTuple(len) => {
        let values = self.stack.split_off(self.stack.len() - len as usize);
        let tuple = self.heap.allocate_tuple(values);
        self.stack.push(tuple);
      }
      Instruction::GetTuple(len) => match self.stack.pop() {
        Some(Value::Heap(handle)) => match self.heap.get(handle) {
          HeapValue::Tuple(values) if (len as usize) < values.len() => {
            let value = values[len as usize];
            self.stack.push(value);
          }
          HeapValue::Tuple(_) => panic!("arity error"),
          HeapValue::Str(_) => panic!("expected tuple"),
        },
        _ => panic!("expected value"),
      },
      Instruction::Add | Instruction::Sub | Instruction::Mul | Instruction::Div => {
        let Value::Int(rhs) = self.stack.pop().expect("rhs") else {
          panic!("not an integer")
        };
        let Value::Int(lhs) = self.stack.pop().expect("lhs") else {
          panic!("not an integer")
        };
        let result = match instruction {
          Instruction::Add => lhs + rhs,
          Instruction::Sub => lhs - rhs,
          Instruction::Mul => lhs * rhs,
          Instruction::Div => lhs / rhs,
          _ => unreachable!(),
        };
        self.stack.push(Value::Int(result));
      }
      Instruction::This => self.stack.push(Value::Pid(self.pid)),
      Instruction::Link => {
        let Value::Pid(pid) = self.stack.pop().expect("target") else {
          panic!("expected pid");
        };

        unsafe {
          let scheduler = self.scheduler.get();
          let other = (*scheduler).processes.get_mut(&pid).expect("exists");
          if !other.links.contains(&self.pid) {
            other.links.push(self.pid);
          }
        }

        if !self.links.contains(&pid) {
          self.links.push(pid);
        }
      }
      Instruction::Monitor => {
        let Value::Pid(pid) = self.stack.pop().expect("target") else {
          panic!("expected pid");
        };

        unsafe {
          let scheduler = self.scheduler.get();
          let other = (*scheduler).processes.get_mut(&pid).expect("exists");
          if !other.monitors.contains(&self.pid) {
            other.monitors.push(self.pid);
          }
        }
      }
      Instruction::Send => {
        pop_or_exit!(self, message);
        let Value::Pid(pid) = self.stack.pop().unwrap() else {
          panic!("expected pid")
        };
        unsafe {
          let scheduler = self.scheduler.get();
          let other = (*scheduler)
            .processes
            .get_mut(&pid)
            .expect("process exists");

          let message = self.heap.deep_clone_at(message, &mut other.heap);
          other.mailbox.push_back(message);

          if other.state == ProcessState::Waiting {
            other.state = ProcessState::Runnable;
            (*scheduler).task_queue.push_back(other.pid);
          }
        }
      }
      Instruction::Receive => match self.mailbox.pop_front() {
        Some(message) => self.stack.push(message),
        None => {
          self.rewind();
          self.state = ProcessState::Waiting;
        }
      },
      Instruction::Store(index) => {
        let frame = self.frames.last_mut().expect("not empty");
        pop_or_exit!(self, value);
        frame.locals[index as usize] = value;
      }
      Instruction::Load(index) => {
        let frame = self.frames.last_mut().expect("not empty");
        let value = frame.locals[index as usize];
        self.stack.push(value);
      }
      Instruction::Goto(index) => {
        self.ip.set(index as usize);
      }
      Instruction::Return => match self.frames.pop() {
        Some(frame) => {
          self.ip = frame.ip;
        }
        None => self.state = ProcessState::Exiting,
      },
      Instruction::ExitReason => {
        self.exit_reason = Some(self.stack.pop().unwrap());
        self.state = ProcessState::Exiting;
      }
      Instruction::Exit => self.state = ProcessState::Exiting,
    };
    self.state
  }
}

#[derive(Debug, Default)]
pub struct Scheduler {
  pub task_queue: VecDeque<Pid>,
  pub processes: BTreeMap<Pid, Process>,
}

thread_local! {
    // TODO: don't like this UnsafeCell thing, search something better?
    static GLOBAL_SCHEDULER: Rc<UnsafeCell<Scheduler>> = Rc::new(UnsafeCell::new(Scheduler::default()));
}

impl Scheduler {
  pub fn add_process(&mut self, process: Process) {
    self.task_queue.push_back(process.pid);
    self.processes.insert(process.pid, process);
  }

  pub fn run(&mut self) {
    while let Some(pid) = self.task_queue.pop_front() {
      let process = match self.processes.get_mut(&pid) {
        Some(process) => process,
        None => panic!("dangling process : {pid:?}"),
      };

      let mut w = 0i32;

      while (w < 8) & (process.state != ProcessState::Exiting) {
        match process.step() {
          ProcessState::Runnable => {
            process.state = ProcessState::Running;
            w += 2;
          }
          ProcessState::Running => w += 2,
          ProcessState::Waiting | ProcessState::Exiting => break,
        }
      }

      match process.state {
        ProcessState::Runnable => {}
        ProcessState::Waiting => {}
        ProcessState::Running => self.task_queue.push_back(pid),
        ProcessState::Exiting => {
          let process = self.processes.remove(&pid).unwrap();

          if process.exit_reason.is_some() {
            let exit_reason = process.exit_reason.unwrap();

            for linked_pid in &process.links {
              // TODO: cascading?
              if let Some(link) = self.processes.get_mut(&linked_pid) {
                eprintln!(
                  "process {:?} exiting due to linked {:?}",
                  link.pid, process.pid
                );

                // TODO: trap exit
                link.exit_reason = Some(process.heap.deep_clone_at(exit_reason, &mut link.heap));

                if link.state != ProcessState::Exiting {
                  link.state = ProcessState::Exiting;
                  self.task_queue.push_front(*linked_pid);
                }
              }
            }
          }

          let mut process = process;

          for monitoring_pid in process.monitors {
            if let Some(monitor) = self.processes.get_mut(&monitoring_pid) {
              let exit = match process.exit_reason {
                Some(reason) => {
                  let exit = process.heap.allocate_tuple(vec![
                    Value::Atom(ATOM_EXIT),
                    Value::Pid(pid),
                    reason,
                  ]);
                  process.heap.deep_clone_at(exit, &mut monitor.heap)
                }
                None => monitor.heap.allocate_tuple(vec![
                  Value::Atom(ATOM_EXIT),
                  Value::Pid(pid),
                  Value::Atom(ATOM_NORMAL),
                ]),
              };

              monitor.mailbox.push_back(exit);

              if monitor.state == ProcessState::Waiting {
                monitor.state = ProcessState::Runnable;
                self.task_queue.push_front(monitor.pid);
              }
            }
          }
          self.task_queue.retain(|e| *e != pid);
        }
      }
    }
  }
}

fn main() {
  tests::program_monitor();
}
