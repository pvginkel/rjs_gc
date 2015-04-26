@echo off
cls & cargo run & gdb --quiet target\release\rjs_gc.exe < gdbscript.txt
