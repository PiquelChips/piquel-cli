package cmd

import (
	"fmt"
	"slices"

	"github.com/PiquelChips/piquel-cli/config"
	"github.com/PiquelChips/piquel-cli/tmux"
	"github.com/spf13/cobra"
)

var (
	listCmd = &cobra.Command{
		Use:     "list [-ct]",
		Short:   "Lists sessions from configuration and tmux",
		Aliases: []string{"ls"},
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			if !tmuxFlag && !configFlag {
				return listSessions(true, true)
			}
			return listSessions(configFlag, tmuxFlag)
		},
	}
	tmuxFlag, configFlag bool
)

func init() {
	rootCmd.AddCommand(listCmd)

	listCmd.Flags().BoolVarP(&configFlag, "config", "c", true, "get sessions from config")
	listCmd.Flags().BoolVarP(&tmuxFlag, "tmux", "t", true, "get sessions from tmux")
}

func listSessions(listConfig, listTmux bool) error {
	sessions := []string{}
	if listTmux {
		tmuxSessions, err := tmux.ListSessions()
		if err != nil {
			return err
		}
		sessions = append(sessions, tmuxSessions...)
	}

	if listConfig {
		for session := range config.Config.Sessions {
			sessions = append(sessions, session)
		}
	}

	slices.Sort(sessions)
	sessions = slices.Compact(sessions)

	for _, session := range sessions {
		fmt.Printf("%s\n", session)
	}

	return nil
}
