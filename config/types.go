package config

type PiquelConfig struct {
	Sessions map[string]SessionConfig `yaml:"sessions"`
}

type SessionConfig struct {
	Root         string                  `yaml:"root"`
	SelectWindow int                     `yaml:"select_window"`
	Windows      []WindowConfig `yaml:"windows"`
}

type WindowConfig struct {
	Name     string   `yaml:"string"`
	Commands []string `yaml:"commands"`
}
