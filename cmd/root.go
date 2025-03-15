package cmd

import (
	"fmt"
	"os"

	"github.com/PiquelChips/piquel-cli/config"
	"github.com/spf13/cobra"
)

var (
	rootCmd = &cobra.Command{
		Use:   "piquel-cli",
		Short: "Piquel's CLI",
		PersistentPreRunE: func(cmd *cobra.Command, args []string) error {
			return config.LoadConfig(configPath)
		},
	}
	configPath string
)

func Execute() {
	err := rootCmd.Execute()
	if err != nil {
		os.Exit(1)
	}
}

func init() {
	userHomeDir, err := os.UserHomeDir()
	if err != nil {
		panic(err)
	}

	rootCmd.PersistentFlags().StringVar(&configPath, "config", fmt.Sprintf("%s/.config/piquel/config.yml", userHomeDir), "config file")
}
