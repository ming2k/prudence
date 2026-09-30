# Disable file completions by default unless specified
complete -c prudence -f
complete -c trash -f

for cmd in prudence trash
    complete -c $cmd -s h -l help -d "Print help information"
    complete -c $cmd -s V -l version -d "Print version information"

    # Subcommands
    complete -c $cmd -n "__fish_use_subcommand" -a list -d "List trashed files and directories"
    complete -c $cmd -n "__fish_use_subcommand" -a clear -d "Permanently empty the trash"
    complete -c $cmd -n "__fish_use_subcommand" -a clean -d "Alias for clear"
    complete -c $cmd -n "__fish_use_subcommand" -a empty -d "Alias for clear"
    complete -c $cmd -n "__fish_use_subcommand" -a restore -d "Restore entries from trash"

    # Default fallback to files for trashing
    complete -c $cmd -n "__fish_use_subcommand" -F

    # restore dynamic completion parsing `prudence list`
    complete -c $cmd -n "__fish_seen_subcommand_from restore" -a \
        '(prudence list 2>/dev/null | string match -r "^\\[(.*)\\]" | string replace -r "^\\[(.*)\\]" \'$1\')'
    complete -c $cmd -n "__fish_seen_subcommand_from restore" -a \
        '(prudence list 2>/dev/null | string match -r "^\s*name:\s*(.*)" | string replace -r "^\s*name:\s*" "")'
end
