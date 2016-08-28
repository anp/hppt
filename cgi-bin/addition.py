#!/usr/bin/env python3

# this is the python example script at https://en.wikipedia.org/wiki/Common_Gateway_Interface
# adapted for python 3

import cgi

import sys

if __name__ == '__main__':
    input_data=cgi.FieldStorage()

    print('Content-Type:text/html\r') #HTML is following
    print('\r')                         #Leave a blank line
    print('<h1>Addition Results</h1>\r')

    try:
        num1 = int(input_data["num1"].value)
        num2 = int(input_data["num2"].value)
    except:
        print('<p>Sorry, we cannot turn your inputs into integers.</p>\r')
        sys.exit(1)

    sum = num1 + num2
    print('<p>{0} + {1} = {2}</p>\r'.format(num1, num2, sum))
