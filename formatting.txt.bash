#!/bin/bash
# This script used to create formatting.txt file for --help.

echo -e "\e[1;4mFormatting:\e[0m"
echo -e "  \e[1m%x %X\e[0m The x-coordinate of the selection"
echo -e "  \e[1m%y %Y\e[0m The y-coordinate of the selection"
echo -e "  \e[1m%w %W\e[0m The width of the selection"
echo -e "  \e[1m%h %H\e[0m The height of the selection"
echo -e "  \e[1m%o   \e[0m The name of output"
echo -e "  \e[1m%n   \e[0m Newline char ('\\\\n')"

