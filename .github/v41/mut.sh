source .github/v41/common.sh
run baseline timeout 3600 cargo test --manifest-path $M --lib terminal::pty::tests::
mut(){ name=$1; old=$2; new=$3
  python3 - "$F" "$old" "$new" <<'PY'
import sys
p,o,n=sys.argv[1:]; s=open(p).read(); c=s.count(o)
if c!=1: sys.exit(f"MUTATION_APPLY_FAILED {c} matches")
open(p,'w').write(s.replace(o,n)); print("mutated")
PY
  [ $? = 0 ] || { echo "$name exit=APPLYFAIL" >> ci-logs/exits.txt; return; }
  git diff -- $F > ci-logs/$name.diff
  run $name timeout 3600 cargo test --manifest-path $M --lib terminal::pty::tests::
  git checkout -- $F
}
# M1: drop normalisation in the resolver (candidate = raw preferred)
mut m1_drop_normalise_resolver 'let candidate = utf8_locale_name(&preferred)?;' 'let candidate = preferred.clone();'
# M2: utf8_locale_name returns input unchanged (no .UTF-8 forcing)
mut m2_drop_normalise_fn 'None => format!("{base}.UTF-8"),' 'None => name.to_string(),'
# M3: ignore /etc/default/locale
mut m3_ignore_default_locale '["/etc/locale.conf", "/etc/default/locale"]' '["/etc/locale.conf"]'
# M4: installed check stops treating utf-8 == utf8
mut m4_no_codeset_fold '.replace("utf-8", "utf8")' '.to_string()'
# M5: localectl never consulted
mut m5_no_localectl '.command_stdout("localectl", &["status"])' '.command_stdout("localectl-disabled", &["status"])'
