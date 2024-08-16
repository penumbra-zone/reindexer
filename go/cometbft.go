package main

import "C"
import "fmt"
import "github.com/cometbft/cometbft/version"

//export printHello
func printHello() {
	fmt.Println("Hello from Go!")
	fmt.Printf("cometbft version %s\n", version.TMCoreSemVer)
}

func main() {}
