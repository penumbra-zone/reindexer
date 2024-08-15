package main

import "C"
import "fmt"

//export printHello
func printHello() {
	fmt.Println("Hello from Go!")
}

func main() {}
