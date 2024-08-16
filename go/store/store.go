package store

import (
	"fmt"

	"github.com/cometbft/cometbft/version"
)

type Store struct {
}

func NewStore(dir string) *Store {
	fmt.Println("dir", dir)
	return &Store{}
}

func (s *Store) MessageA() {
	fmt.Println("Go: A!")
}

func (s *Store) MessageB() {
	fmt.Printf("Go: B, version: %s\n", version.TMCoreSemVer)
}
