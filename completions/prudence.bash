_prudence() {
    local cur prev words cword
    _init_completion || return

    local commands="list clear clean empty restore"

    if [[ $cword -eq 1 ]]; then
        if [[ "$cur" == -* ]]; then
            COMPREPLY=( $(compgen -W "-h --help -V --version --" -- "$cur") )
        else
            COMPREPLY=( $(compgen -W "$commands" -- "$cur") $(compgen -f -- "$cur") )
        fi
        return 0
    fi

    case "${words[1]}" in
        restore)
            local entries
            entries=$(prudence list 2>/dev/null | awk '/^\[/{gsub(/[\[\]]/, ""); print} /^  name:/{print $2}')
            COMPREPLY=( $(compgen -W "$entries" -- "$cur") )
            return 0
            ;;
        list|clear|clean|empty)
            return 0
            ;;
        *)
            _filedir
            return 0
            ;;
    esac
}

complete -F _prudence prudence trash
