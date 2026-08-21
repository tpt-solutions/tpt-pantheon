package main

import (
	"fmt"
	"os"

	"github.com/PhillipC05/tpt-identity/cmd/tpt-identity/cmd"
)

func main() {
	if err := cmd.Root().Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
