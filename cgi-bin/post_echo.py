#!/usr/bin/env python3

import sys

if __name__ == '__main__':
    input = sys.stdin.buffer.read()
    input = input.decode()
    print('\r\n', end='')
    print(input, end='')
