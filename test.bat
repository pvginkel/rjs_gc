@echo off
cls & cargo run & gdb --quiet target\debug\rjs_gc.exe < gdbscript.txt
