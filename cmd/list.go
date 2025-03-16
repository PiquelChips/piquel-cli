package cmd

import (
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
			// If no flag is specified
			if !tmuxFlag && !configFlag {
				return tmux.ListSessions(true, true)
			}
			return tmux.ListSessions(configFlag, tmuxFlag)
		},
	}
	tmuxFlag, configFlag bool
)

func init() {
	rootCmd.AddCommand(listCmd)

	listCmd.Flags().BoolVarP(&configFlag, "config", "c", true, "get sessions from config")
	listCmd.Flags().BoolVarP(&tmuxFlag, "tmux", "t", true, "get sessions from tmux")
}
