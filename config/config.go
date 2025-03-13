package config

import (
	"fmt"
	"os"

	"gopkg.in/yaml.v3"
)

var Config PiquelConfig
var configLoaded bool = false

func LoadConfig(configPath string) {
    if configLoaded {
        panic(fmt.Errorf("Config has already been loaded from %s", configPath))
    }

    configFile, err := os.ReadFile(configPath)
    if err != nil {
        panic(fmt.Errorf("Config file %s does not exist", configPath))
    }

    yaml.Unmarshal(configFile, &Config)
}
