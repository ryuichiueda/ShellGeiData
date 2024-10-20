//SPDX-FileCopyrightText: 2024 Ryuichi Ueda ryuichiueda@gmail.com
//SPDX-License-Identifier: BSD-3-Clause

use crate::{utils::error, ShellCore, Feeder};
use crate::utils::{file_check, glob};
use crate::elements::word::Word;
use super::arithmetic::word;
use super::arithmetic::elem::ArithElem;
use std::env;

#[derive(Debug, Clone)]
pub enum CondElem {
    UnaryOp(String),
    BinaryOp(String),
    Word(Word),
    Operand(String),
    InParen(ConditionalExpr),
    Not, // !
    And, // &&
    Or, // ||
    Ans(bool),
}

fn op_order(op: &CondElem) -> u8 {
    match op {
        CondElem::UnaryOp(_) => 14,
        CondElem::BinaryOp(_) => 13,
        CondElem::Not => 12,
        //CondElem::And | CondElem::Or => 12,
        _ => 0,
    }
}

pub fn to_string(op: &CondElem) -> String {
    match op {
        CondElem::UnaryOp(op) => op.to_string(),
        CondElem::BinaryOp(op) => op.to_string(),
        CondElem::InParen(expr) => expr.text.clone(),
        CondElem::Word(w) => w.text.clone(),
        CondElem::Operand(op) => op.to_string(),
        CondElem::Not => "!".to_string(),
        CondElem::And => "&&".to_string(),
        CondElem::Or => "||".to_string(),
        CondElem::Ans(true) => "true".to_string(),
        CondElem::Ans(false) => "false".to_string(),
    }
}

fn to_operand(w: &Word, core: &mut ShellCore) -> Result<CondElem, String> {
    match w.eval_for_case_pattern(core) {
        Some(v) => Ok(CondElem::Operand(v)),
        None => return Err(format!("{}: wrong substitution", &w.text)),
    }
}

fn pop_operand(stack: &mut Vec<CondElem>, core: &mut ShellCore) -> Result<CondElem, String> {
    match stack.pop() {
        Some(CondElem::InParen(mut expr)) => expr.eval(core),
        Some(CondElem::Word(w)) => to_operand(&w, core),
        Some(elem) => Ok(elem),
        None => return Err("no operand".to_string()),
    }
}

#[derive(Debug, Clone)]
pub struct ConditionalExpr {
    pub text: String,
    elements: Vec<CondElem>,
}

impl ConditionalExpr {
    pub fn eval(&mut self, core: &mut ShellCore) -> Result<CondElem, String> {
        let mut from = 0;
        let mut next = true;
        let mut last = CondElem::Ans(true);
        for i in 0..self.elements.len() {
            match self.elements[i] {
                CondElem::And | CondElem::Or => {
                    if next {
                        last = match Self::calculate(&self.elements[from..i], core) {
                            Ok(elem) => elem, 
                            Err(e)   => return Err(e),
                        };
                    }
                    from = i + 1;

                    next = match (&self.elements[i], &last) {
                        (CondElem::And, CondElem::Ans(ans)) => *ans,
                        (CondElem::Or, CondElem::Ans(ans))  => !ans,
                        _ => panic!("SUSH INTERNAL ERROR"),
                    };
                },
                _ => {},
            }
        }
 
        Ok(last)
    }

    fn calculate(elems: &[CondElem], core: &mut ShellCore) -> Result<CondElem, String> {
        let rev_pol = match Self::rev_polish(elems) {
            Ok(ans) => ans,
            Err(e) => return Err(e),
        };
        let mut stack = match Self::reduce(&rev_pol, core) {
            Ok(s)  => s, 
            Err(e) => return Err(e),
        };
    
        match pop_operand(&mut stack, core) {
            Ok(CondElem::Operand(s))  => Ok(CondElem::Ans(s.len() > 0)), //for [[ string ]]
            other_ans             => other_ans,
        }
    }

    fn rev_polish(elems: &[CondElem]) -> Result<Vec<CondElem>, String> {
        let mut ans = vec![];
        let mut stack = vec![];
    
        for e in elems {
            let ok = match e {
                CondElem::Word(_) | CondElem::InParen(_) => {ans.push(e.clone()); true},
                op               => Self::rev_polish_op(&op, &mut stack, &mut ans),
            };
    
            if !ok {
                let msg = "syntax error near ".to_owned() + &to_string(e);
                return Err(msg);
            }
        }
    
        while stack.len() > 0 {
            ans.push(stack.pop().unwrap());
        }
    
        Ok(ans)
    }

    fn reduce(rev_pol: &[CondElem], core: &mut ShellCore) -> Result<Vec<CondElem>, String> {
        let mut stack = vec![];

        for e in rev_pol {
            let result = match e { 
                CondElem::Word(_) | CondElem::InParen(_) => {
                    stack.push(e.clone());
                    Ok(())
                },
                CondElem::UnaryOp(ref op) => Self::unary_operation(&op, &mut stack, core),
                CondElem::BinaryOp(ref op) => {
                    if stack.len() == 0 {
                        return Ok(vec![CondElem::Ans(true)]); //for [[ -ot ]] [[ == ]] [[ = ]] ...
                    }
                    Self::bin_operation(&op, &mut stack, core)
                },
                CondElem::Not => match pop_operand(&mut stack, core) {
                    Ok(CondElem::Ans(res)) => {
                        stack.push(CondElem::Ans(!res));
                        Ok(())
                    },
                    _ => Err("no operand to negate".to_string()),
                },
                _ => Err( error::syntax("TODO")),
            };
    
            if let Err(err_msg) = result {
                core.data.set_param("?", "2");
                return Err(err_msg);
            }
        }

        if stack.len() != 1 { 
            let mut err = "syntax error".to_string();
            if stack.len() > 1 {
                err = error::syntax_in_cond_expr(&to_string(&stack[0]));
                error::print(&err, core, true);
                err = format!("syntax error near `{}'", to_string(&stack[0]));
            }
            return Err(err);
        }   

        Ok(stack)
    }

    fn unary_operation(op: &str, stack: &mut Vec<CondElem>, core: &mut ShellCore) -> Result<(), String> {
        let operand = match pop_operand(stack, core) {
            Ok(CondElem::Operand(v))  => v,
            Ok(_)  => return Err("unknown operand".to_string()), 
            Err(e) => return Err(e + " to conditional unary operator"),
        };

        if op == "-o" || op == "-v" || op == "-z" || op == "-n" {
            let ans = match op {
                "-o" => core.options.query(&operand),
                "-v" => core.data.get_value(&operand).is_some() || env::var(&operand).is_ok(),
                "-z" => operand.len() == 0,
                "-n" => operand.len() > 0,
                _    => false,
            };

            stack.push( CondElem::Ans(ans) );
            return Ok(());
        }

        Self::unary_file_check(op, &operand, stack)
    }

    fn bin_operation(op: &str, stack: &mut Vec<CondElem>, core: &mut ShellCore) -> Result<(), String> {
        let right = match pop_operand(stack, core) {
            Ok(CondElem::Operand(name)) => name,
            Ok(_)  => return Err("Invalid operand".to_string()),
            Err(e) => return Err(e),
        };
    
        let left = match pop_operand(stack, core) {
            Ok(CondElem::Operand(name)) => name,
            Ok(_)  => return Err("Invalid operand".to_string()),
            Err(e) => return Err(e),
        };

        let extglob = core.shopts.query("extglob");
        if op == "==" || op == "=" || op == "!=" || op == "<" || op == ">" {
            let ans = match op {
                "==" | "=" => glob::compare(&left, &right, extglob),
                "!="       => ! glob::compare(&left, &right, extglob),
                ">"        => left > right,
                "<"        => left < right,
                _    => false,
            };

            stack.push( CondElem::Ans(ans) );
            return Ok(());
        }

        if op == "-eq" || op == "-ne" || op == "-lt" || op == "-le" || op == "-gt" || op == "-ge" {
            let lnum = match word::str_to_num(&left, core) {
                Ok(ArithElem::Integer(n)) => n,
                Ok(_) => return Err("non integer number is not supported".to_string()),
                Err(msg) => return Err(msg),
            };
            let rnum = match word::str_to_num(&right, core) {
                Ok(ArithElem::Integer(n)) => n,
                Ok(_) => return Err("non integer number is not supported".to_string()),
                Err(msg) => return Err(msg),
            };

            let ans = match op {
                "-eq" => lnum == rnum,
                "-ne" => lnum != rnum,
                "-lt" => lnum < rnum,
                "-le" => lnum <= rnum,
                "-gt" => lnum > rnum,
                "-ge" => lnum >= rnum,
                _    => false,
            };

            stack.push( CondElem::Ans(ans) );
            return Ok(());
        }

        let result = file_check::metadata_comp(&left, &right, op);
        stack.push( CondElem::Ans(result) );
        Ok(())
    }

    fn unary_file_check(op: &str, s: &String, stack: &mut Vec<CondElem>) -> Result<(), String> {
        let result = match op {
            "-a" | "-e"  => file_check::exists(s),
            "-d"  => file_check::is_dir(s),
            "-f"  => file_check::is_regular_file(s),
            "-h" | "-L"  => file_check::is_symlink(s),
            "-r"  => file_check::is_readable(s),
            "-t"  => file_check::is_tty(s),
            "-w"  => file_check::is_writable(s),
            "-x"  => file_check::is_executable(s),
            "-b" | "-c" | "-g" | "-k" | "-p" | "-s" | "-u" | "-G" | "-N" | "-O" | "-S"
                  => file_check::metadata_check(s, op),
            _  => return Err("unsupported option".to_string()),
        };

        stack.push( CondElem::Ans(result) );
        Ok(())
    }

    fn rev_polish_op(elem: &CondElem,
                     stack: &mut Vec<CondElem>, ans: &mut Vec<CondElem>) -> bool {
        loop {
            match stack.last() {
                None => {
                    stack.push(elem.clone());
                    break;
                },
                Some(_) => {
                    let last = stack.last().unwrap();
                    if op_order(last) <= op_order(elem) {
                        stack.push(elem.clone());
                        break;
                    }
                    ans.push(stack.pop().unwrap());
                },
            }
        }
    
        true
    }

    fn new() -> ConditionalExpr {
        ConditionalExpr {
            text: String::new(),
            elements: vec![],
        }
    }

    fn eat_word(feeder: &mut Feeder, ans: &mut Self, core: &mut ShellCore) -> bool {
        if feeder.starts_with("]]")
        || feeder.starts_with(")")
        || feeder.starts_with("(") {
            return false;
        }

        match Word::parse(feeder, core, false) {
            Some(w) => {
                ans.text += &w.text.clone();
                ans.elements.push(CondElem::Word(w));

                true
            },
            _ => false
        }
    }

    fn eat_compare_op(feeder: &mut Feeder, ans: &mut Self, core: &mut ShellCore) -> bool {
        let len = feeder.scanner_test_compare_op(core);
        if len == 0 {
            return false;
        }

        let opt = feeder.consume(len);
        ans.text += &opt.clone();
        ans.elements.push(CondElem::BinaryOp(opt));

        true
    }

    fn eat_file_check_option(feeder: &mut Feeder, 
                             ans: &mut Self,
                             core: &mut ShellCore) -> bool {
        let len = feeder.scanner_test_check_option(core);
        if len == 0 {
            return false;
        }

        let opt = feeder.consume(len);
        ans.text += &opt.clone();
        ans.elements.push(CondElem::UnaryOp(opt));

        true
    }

    fn eat_not_and_or(feeder: &mut Feeder, ans: &mut Self) -> bool {
        if feeder.starts_with("!") {
            ans.text += &feeder.consume(1);
            ans.elements.push( CondElem::Not );
            return true;
        }
        if feeder.starts_with("&&") {
            ans.text += &feeder.consume(2);
            ans.elements.push( CondElem::And );
            return true;
        }
        if feeder.starts_with("||") {
            ans.text += &feeder.consume(2);
            ans.elements.push( CondElem::Or );
            return true;
        }

        false
    }

    fn eat_paren(feeder: &mut Feeder, ans: &mut Self, 
                 core: &mut ShellCore) -> bool {
        if let Some(e) = ans.elements.last() {
            match e {
                CondElem::UnaryOp(_) => {
                    return false
                },
                _ => {},
            }
        }

        if ! feeder.starts_with("(") {
            return false;
        }

        ans.text += &feeder.consume(1);

        let expr = match Self::parse(feeder, core) {
            Some(e) => e,
            None    => return false,
        };

        if ! feeder.starts_with(")") {
            return false;
        }

        ans.text += &expr.text.clone();
        ans.elements.push( CondElem::InParen(expr) );
        ans.text += &feeder.consume(1);
        true
    }

    fn eat_blank(feeder: &mut Feeder, ans: &mut Self, core: &mut ShellCore) -> bool {
        match feeder.scanner_blank(core) {
            0 => false,
            n => {
                ans.text += &feeder.consume(n);
                true
            },
        }
    }

    pub fn parse(feeder: &mut Feeder, core: &mut ShellCore) -> Option<Self> {
        let mut ans = Self::new();

        loop {
            Self::eat_blank(feeder, &mut ans, core);
            if feeder.starts_with("]]")
            || feeder.starts_with(")") {
                if ans.elements.len() == 0 {
                    return None;
                }

                ans.elements.push(CondElem::And);
                return Some(ans);
            }

            if Self::eat_paren(feeder, &mut ans, core) 
            || Self::eat_compare_op(feeder, &mut ans, core)
            || Self::eat_file_check_option(feeder, &mut ans, core)
            || Self::eat_not_and_or(feeder, &mut ans) 
            || Self::eat_word(feeder, &mut ans, core) {
                continue;
            }

            let fnn = "dummy";
            break;
        }
        None
    }
}
