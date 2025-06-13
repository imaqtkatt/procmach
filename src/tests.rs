use std::{cell::Cell, rc::Rc};

use crate::{
  Frame, GLOBAL_SCHEDULER, Heap, Instruction, Mailbox, Pid, Process, ProcessState, Value,
};

#[allow(dead_code)]
pub fn program_monitor() {
  let process_exit = {
    let this_frame = Frame {
      ip: Cell::default(),
      code: vec![Instruction::PushInt(3), Instruction::Sub, Instruction::Exit],
      locals: vec![],
    };
    let frame_return_zero = Frame {
      ip: Cell::default(),
      code: vec![Instruction::PushInt(0), Instruction::Return],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![this_frame, frame_return_zero],
      mailbox: Mailbox::default(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
      exit_reason: None,
    }
  };

  let process_main = {
    let this_frame = Frame {
      ip: Cell::default(),
      code: vec![
        Instruction::PushPid(process_exit.pid),
        Instruction::Monitor,
        Instruction::Receive,
        Instruction::DbgDrop,
        Instruction::Goto(3),
        Instruction::Exit,
      ],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![this_frame],
      mailbox: Mailbox::default(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
      exit_reason: None,
    }
  };

  let scheduler = GLOBAL_SCHEDULER.with(Rc::clone);

  unsafe {
    let scheduler = scheduler.get();
    (*scheduler).add_process(process_main);
    (*scheduler).add_process(process_exit);
    (*scheduler).run();
  }
}

#[allow(dead_code)]
pub fn program_adder() {
  let process_main = {
    let main_frame = Frame {
      ip: Cell::default(),
      code: vec![
        Instruction::Receive,
        Instruction::Store(0),
        Instruction::Receive,
        Instruction::Store(1),
        Instruction::Load(0),
        Instruction::Load(1),
        Instruction::Add,
        Instruction::DbgStack,
        Instruction::Exit,
      ],
      locals: vec![Value::Int(69), Value::Int(69)],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![main_frame],
      mailbox: Mailbox::default(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
      exit_reason: None,
    }
  };

  let process_sender = {
    let frame = Frame {
      ip: Cell::default(),
      code: vec![
        Instruction::PushPid(process_main.pid),
        Instruction::PushInt(3),
        Instruction::Send,
        Instruction::PushPid(process_main.pid),
        Instruction::PushInt(3),
        Instruction::Send,
        Instruction::Exit,
      ],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![frame],
      mailbox: Mailbox::default(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
      exit_reason: None,
    }
  };

  let scheduler = GLOBAL_SCHEDULER.with(Rc::clone);

  unsafe {
    let scheduler = scheduler.get();
    (*scheduler).add_process(process_main);
    (*scheduler).add_process(process_sender);
    (*scheduler).run();
  }
}

#[allow(dead_code)]
pub fn program_link() {
  let process_exit1 = {
    let this_frame = Frame {
      ip: Cell::default(),
      code: vec![Instruction::PushInt(99), Instruction::ExitReason],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![this_frame],
      mailbox: Mailbox::default(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
      exit_reason: None,
    }
  };

  let process_exit2 = {
    let this_frame = Frame {
      ip: Cell::default(),
      code: vec![
        Instruction::PushPid(process_exit1.pid),
        Instruction::Link,
        Instruction::Goto(2),
        Instruction::Exit,
      ],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![this_frame],
      mailbox: Mailbox::default(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
      exit_reason: None,
    }
  };

  let process_main = {
    let main_frame = Frame {
      ip: Cell::default(),
      code: vec![
        Instruction::PushPid(process_exit2.pid),
        Instruction::Link,
        Instruction::Goto(2),
        Instruction::Exit,
      ],
      locals: vec![],
    };
    Process {
      pid: Pid::next(),
      ip: Cell::new(0),
      state: ProcessState::Runnable,
      stack: vec![],
      frames: vec![main_frame],
      mailbox: Mailbox::default(),
      scheduler: GLOBAL_SCHEDULER.with(Rc::clone),
      links: vec![],
      monitors: vec![],
      heap: Heap::default(),
      exit_reason: None,
    }
  };

  let scheduler = GLOBAL_SCHEDULER.with(Rc::clone);

  unsafe {
    let scheduler = scheduler.get();
    (*scheduler).add_process(process_main);
    (*scheduler).add_process(process_exit2);
    (*scheduler).add_process(process_exit1);
    (*scheduler).run();
  }
}
