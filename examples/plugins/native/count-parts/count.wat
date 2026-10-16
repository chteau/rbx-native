;; Walks Workspace with instance.get and counts the instances that have an
;; Anchored property (only BaseParts do; instance.property errors otherwise).
;;
;; Memory map: 0..1023 constant strings (NUL-terminated), 4096 request buffer,
;; 65536..262143 stack of refs still to visit, 262144.. host replies.
(module
  (import "rbxn" "call" (func $host (param i32 i32) (result i64)))
  (memory (export "memory") 32)

  (data (i32.const 0) "count\00")
  (data (i32.const 16) "{\"method\":\"command.register\",\"params\":{\"id\":\"count\",\"text\":\"Count parts\",\"tooltip\":\"Log how many BaseParts are in Workspace\"}}\00")
  (data (i32.const 256) "{\"method\":\"instance.find\",\"params\":{\"path\":\"Workspace\"}}\00")
  (data (i32.const 384) "{\"method\":\"instance.get\",\"params\":{\"ref\":\00")
  (data (i32.const 512) "{\"method\":\"instance.property\",\"params\":{\"name\":\"Anchored\",\"ref\":\00")
  (data (i32.const 640) "}}\00")
  (data (i32.const 656) "\"children\":[\00")
  (data (i32.const 704) "\"ok\":\00")
  (data (i32.const 720) "{\"method\":\"log\",\"params\":{\"text\":\"BaseParts in Workspace: \00")
  (data (i32.const 832) "\"}}\00")
  (data (i32.const 896) "{\"method\":\"warn\",\"params\":{\"text\":\"count: Workspace not found\"}}\00")

  (global $rp (mut i32) (i32.const 0))
  (global $rl (mut i32) (i32.const 0))
  (global $cur (mut i32) (i32.const 0))

  (func $cat (param $d i32) (param $s i32) (result i32)
    (local $c i32)
    (block $done
      (loop $l
        (br_if $done (i32.eqz (local.tee $c (i32.load8_u (local.get $s)))))
        (i32.store8 (local.get $d) (local.get $c))
        (local.set $d (i32.add (local.get $d) (i32.const 1)))
        (local.set $s (i32.add (local.get $s) (i32.const 1)))
        (br $l)))
    (local.get $d))

  (func $utoa (param $d i32) (param $n i32) (result i32)
    (local $digits i32) (local $t i32) (local $e i32)
    (local.set $t (local.get $n))
    (loop $count
      (local.set $digits (i32.add (local.get $digits) (i32.const 1)))
      (local.set $t (i32.div_u (local.get $t) (i32.const 10)))
      (br_if $count (local.get $t)))
    (local.set $e (i32.add (local.get $d) (local.get $digits)))
    (local.set $t (local.get $e))
    (loop $write
      (local.set $t (i32.sub (local.get $t) (i32.const 1)))
      (i32.store8 (local.get $t)
        (i32.add (i32.const 48) (i32.rem_u (local.get $n) (i32.const 10))))
      (local.set $n (i32.div_u (local.get $n) (i32.const 10)))
      (br_if $write (i32.lt_u (local.get $d) (local.get $t))))
    (local.get $e))

  ;; Address just past the first occurrence of the NUL-terminated needle in
  ;; hay[0..len), or 0.
  (func $find (param $h i32) (param $len i32) (param $n i32) (result i32)
    (local $i i32) (local $j i32)
    (block $out
      (loop $outer
        (br_if $out (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $j (i32.const 0))
        (block $miss
          (loop $inner
            (if (i32.eqz (i32.load8_u (i32.add (local.get $n) (local.get $j))))
              (then (return
                (i32.add (i32.add (local.get $h) (local.get $i)) (local.get $j)))))
            (br_if $miss (i32.ge_u (i32.add (local.get $i) (local.get $j)) (local.get $len)))
            (br_if $miss (i32.ne
              (i32.load8_u (i32.add (i32.add (local.get $h) (local.get $i)) (local.get $j)))
              (i32.load8_u (i32.add (local.get $n) (local.get $j)))))
            (local.set $j (i32.add (local.get $j) (i32.const 1)))
            (br $inner)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $outer)))
    (i32.const 0))

  (func $digit (param $c i32) (result i32)
    (i32.lt_u (i32.sub (local.get $c) (i32.const 48)) (i32.const 10)))

  ;; Decimal number at p (stopping at end); $cur is left after its digits.
  (func $num (param $p i32) (param $end i32) (result i32)
    (local $v i32)
    (block $out
      (loop $l
        (br_if $out (i32.ge_u (local.get $p) (local.get $end)))
        (br_if $out (i32.eqz (call $digit (i32.load8_u (local.get $p)))))
        (local.set $v (i32.add
          (i32.mul (local.get $v) (i32.const 10))
          (i32.sub (i32.load8_u (local.get $p)) (i32.const 48))))
        (local.set $p (i32.add (local.get $p) (i32.const 1)))
        (br $l)))
    (global.set $cur (local.get $p))
    (local.get $v))

  ;; Sends the request in 4096..end; the reply lands in $rp/$rl.
  (func $ask (param $end i32)
    (local $r i64)
    (local.set $r (call $host (i32.const 4096)
      (i32.sub (local.get $end) (i32.const 4096))))
    (global.set $rp (i32.wrap_i64 (i64.shr_u (local.get $r) (i64.const 32))))
    (global.set $rl (i32.wrap_i64 (local.get $r))))

  (func (export "rbxn_alloc") (param $len i32) (result i32)
    (i32.const 262144))

  (func (export "rbxn_start")
    (call $ask (call $cat (i32.const 4096) (i32.const 16))))

  (func (export "rbxn_command") (param $ptr i32) (param $len i32)
    (local $sp i32) (local $count i32) (local $q i32) (local $r i32)
    (local $e i32) (local $end i32) (local $c i32)
    (if (i32.ne (local.get $len) (i32.const 5)) (then (return)))
    (if (i32.eqz (call $find (local.get $ptr) (local.get $len) (i32.const 0)))
      (then (return)))

    (call $ask (call $cat (i32.const 4096) (i32.const 256)))
    (local.set $end (i32.add (global.get $rp) (global.get $rl)))
    (local.set $q (call $find (global.get $rp) (global.get $rl) (i32.const 704)))
    (if (i32.eqz (local.get $q))
      (then
        (call $ask (call $cat (i32.const 4096) (i32.const 896)))
        (return)))
    (local.set $r (call $num (local.get $q) (local.get $end)))
    (if (i32.eq (global.get $cur) (local.get $q))
      (then
        (call $ask (call $cat (i32.const 4096) (i32.const 896)))
        (return)))

    (local.set $sp (i32.const 65536))
    (i32.store (local.get $sp) (local.get $r))
    (local.set $sp (i32.add (local.get $sp) (i32.const 4)))

    (block $empty
      (loop $visit
        (br_if $empty (i32.le_u (local.get $sp) (i32.const 65536)))
        (local.set $sp (i32.sub (local.get $sp) (i32.const 4)))
        (local.set $r (i32.load (local.get $sp)))

        (local.set $e (call $cat (i32.const 4096) (i32.const 512)))
        (local.set $e (call $utoa (local.get $e) (local.get $r)))
        (call $ask (call $cat (local.get $e) (i32.const 640)))
        (if (i32.eq (i32.load8_u (i32.add (global.get $rp) (i32.const 2))) (i32.const 111))
          (then (local.set $count (i32.add (local.get $count) (i32.const 1)))))

        (local.set $e (call $cat (i32.const 4096) (i32.const 384)))
        (local.set $e (call $utoa (local.get $e) (local.get $r)))
        (call $ask (call $cat (local.get $e) (i32.const 640)))
        (local.set $end (i32.add (global.get $rp) (global.get $rl)))
        (local.set $q (call $find (global.get $rp) (global.get $rl) (i32.const 656)))
        (if (local.get $q)
          (then
            (block $list
              (loop $items
                (br_if $list (i32.ge_u (local.get $q) (local.get $end)))
                (local.set $c (i32.load8_u (local.get $q)))
                (br_if $list (i32.eq (local.get $c) (i32.const 93)))
                (if (call $digit (local.get $c))
                  (then
                    (local.set $r (call $num (local.get $q) (local.get $end)))
                    (local.set $q (global.get $cur))
                    (if (i32.lt_u (local.get $sp) (i32.const 262144))
                      (then
                        (i32.store (local.get $sp) (local.get $r))
                        (local.set $sp (i32.add (local.get $sp) (i32.const 4))))))
                  (else
                    (local.set $q (i32.add (local.get $q) (i32.const 1)))))
                (br $items)))))
        (br $visit)))

    (local.set $e (call $cat (i32.const 4096) (i32.const 720)))
    (local.set $e (call $utoa (local.get $e) (local.get $count)))
    (call $ask (call $cat (local.get $e) (i32.const 832)))))
