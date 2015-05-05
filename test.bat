@echo off
cls & cargo build & gdb --quiet target\debug\rjs_gc.exe < gdbscript.txt
