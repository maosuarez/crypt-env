interface CmdHLProps {
  cmd: string;
}

export function CmdHL({ cmd }: CmdHLProps) {
  const parts = cmd.split(/(\{\{[^}]+\}\})/);
  return (
    <>
      {parts.map((p, i) =>
        /^\{\{/.test(p) ? (
          <span
            key={i}
            className="text-warn bg-warn-b rounded-[2px] px-[2px]"
          >
            {p}
          </span>
        ) : (
          <span key={i}>{p}</span>
        )
      )}
    </>
  );
}
