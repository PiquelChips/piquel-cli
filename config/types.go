package config

type PiquelConfig struct {
	Sessions map[string]*SessionConfig `yaml:"sessions"`
}

type SessionConfig struct {
	Root         string          `yaml:"root"`
	Windows      []*WindowConfig `yaml:"windows"`
}

type WindowConfig struct {
	Commands []string `yaml:"commands"`
}
