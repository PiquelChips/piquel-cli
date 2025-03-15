package cmd

import (
	"fmt"
	"slices"

	"github.com/PiquelChips/piquel-cli/config"
	"github.com/PiquelChips/piquel-cli/tmux"
	"github.com/spf13/cobra"
)

// listCmd represents the list command
var listCmd = &cobra.Command{
	Use:     "list [-ct]",
	Short:   "Lists sessions from configuration and tmux",
	Aliases: []string{"ls"},
	Args:    cobra.NoArgs,
	RunE: func(cmd *cobra.Command, args []string) error {
		sessions, _ := tmux.ListSessions()

		for session := range config.Config.Sessions {
			sessions = append(sessions, session)
		}

		slices.Sort(sessions)
		sessions = slices.Compact(sessions)

		for _, session := range sessions {
			fmt.Printf("%s\n", session)
		}

		return nil
	},
}

func init() {
	rootCmd.AddCommand(listCmd)
}
