package utils

import (
	"os"
	"strings"
)

func ExpandHome(path string) string {
	if strings.HasPrefix(path, "~") {
		homeDir, err := os.UserHomeDir()
		if err != nil {
			panic(err)
		}
		return strings.Replace(path, "~", homeDir, 1)
	}
	return path
}
