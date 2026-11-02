# A amostra de PRs da bancada (AMOSTRA=prs no ferramentas/bancada.sh): o projeto
# top-bar-app-color com a pasta em modo PR e um chat para cada estado do PR (checks rodando com
# falhas e mudanças pedidas, Ship ligado, conflito, atrás de main, rascunho, bloqueado, commit de
# fora com UNSTABLE e o "Confirmar", sem permissão, pronto para mesclar, mesclado, fechado, pronto
# para atualizar e o que ainda vai abrir o PR), os itens de "Esperando você" e as issues do GitHub.
# Grava, na pasta da bancada, o estado.json e as conversas, a configuração e o github.json que o
# ferramentas/amostra-prs/gh responde, coerente com o estado (o vigia consulta e nada muda). Sem
# rede: o repositório é local, e o fetch do remote do GitHub vai para um bare ao lado
# (url.<bare>.insteadOf).
# Uso: python3 -I preparar.py <pasta da bancada ($XDG_CACHE_HOME/vitral-bancada)>
import datetime, json, os, shutil, subprocess, sys, time

B = os.path.abspath(sys.argv[1])
D = os.path.join(B, "data", "vitral")
A = os.path.join(B, "amostra")
GH = "VictorMenegate/top-bar-app-color"
LEITURA = "VictorMenegate/so-leitura"
PROJ = "pamostra01"
REPO = os.path.join(A, "top-bar-app-color")
BARE = os.path.join(A, "top-bar-app-color.git")
COPIAS = os.path.join(D, "copias", PROJ)
agora = int(time.time())
iso = lambda segundos_atras: datetime.datetime.fromtimestamp(agora - segundos_atras, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
sha = lambda n: f"{n:07x}" + "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8"[: 40 - 7]


def git(*a, cwd=REPO):
    ambiente = dict(os.environ, GIT_AUTHOR_NAME="Demo", GIT_AUTHOR_EMAIL="demo@vitral", GIT_COMMITTER_NAME="Demo",
                    GIT_COMMITTER_EMAIL="demo@vitral")
    return subprocess.run(["git", *a], cwd=cwd, check=True, capture_output=True, text=True, env=ambiente).stdout.strip()


# ---------- o repositório, local ----------
# Os dados de antes da bancada saem (o app/ que a atualização instalou fica).
shutil.rmtree(A, ignore_errors=True)
for nome in os.listdir(D) if os.path.isdir(D) else []:
    if nome != "app":
        caminho = os.path.join(D, nome)
        shutil.rmtree(caminho) if os.path.isdir(caminho) and not os.path.islink(caminho) else os.remove(caminho)
os.makedirs(REPO)
git("init", "-q", "-b", "main")
open(os.path.join(REPO, "extension.js"), "w").write("""import Meta from 'gi://Meta';

export default class TopBarAppColor {
    enable() {
        this._sinal = global.display.connect('notify::focus-window', () => this._pintar());
        this._pintar();
    }

    disable() {
        global.display.disconnect(this._sinal);
        this._sinal = null;
    }

    _pintar() {
        const janela = global.display.focus_window;
        if (!janela)
            return;
        Main.panel.set_style(`background-color: ${this._corDoApp(janela)};`);
    }
}
""")
open(os.path.join(REPO, "metadata.json"), "w").write('{"uuid": "top-bar-app-color@victor", "shell-version": ["50"]}\n')
git("add", "-A")
git("commit", "-qm", "extensão")
base = git("rev-parse", "HEAD")
git("clone", "-q", "--bare", REPO, BARE, cwd=A)
git("remote", "add", "origin", f"https://github.com/{GH}.git")
git("config", f"url.{BARE}.insteadOf", f"https://github.com/{GH}.git")
git("fetch", "-q", "origin")


def copia(n, ramo, mudar):
    pasta = os.path.join(COPIAS, f"t{n}")
    os.makedirs(COPIAS, exist_ok=True)
    git("worktree", "add", "-q", "-b", ramo, pasta, base)
    mudar(pasta)
    return pasta


def mudar_t1(p):
    s = open(os.path.join(p, "extension.js")).read()
    s = s.replace("        global.display.disconnect(this._sinal);\n        this._sinal = null;",
                  "        if (this._sinal) {\n            global.display.disconnect(this._sinal);\n            this._sinal = null;\n        }\n        Main.panel.set_style(null);")
    open(os.path.join(p, "extension.js"), "w").write(s)


def mudar_t12(p):
    s = open(os.path.join(p, "extension.js")).read()
    s = s.replace("        if (!janela)\n            return;", "        if (!janela) {\n            Main.panel.set_style(null);\n            return;\n        }")
    open(os.path.join(p, "extension.js"), "w").write(s)
    open(os.path.join(p, "novo-cores.js"), "w").write("export const CORES = {\n    firefox: '#ff7139',\n    code: '#007acc',\n};\n")


t1 = copia(1, "vitral/t1-eng-162-sentry", mudar_t1)
t12 = copia(12, "vitral/t12-cor-da-barra-sem-janela", mudar_t12)

# ---------- o estado e o que o GitHub diria, juntos ----------
github = {
    "login": {"error": None, "login": "VictorMenegate", "scopes": "gist, read:org, repo, workflow", "state": "success"},
    "repos": {}, "workflows": {GH: 1, LEITURA: 0}, "ramos": {GH: ["main"], LEITURA: ["main"]},
    "prs": {}, "logs": {}, "issues": {}, "issue": {},
}
for nome, permissao in ((GH, "ADMIN"), (LEITURA, "READ")):
    github["repos"][nome] = {
        "defaultBranchRef": {"name": "main"}, "deleteBranchOnMerge": False, "hasIssuesEnabled": True, "isFork": False,
        "isPrivate": False, "mergeCommitAllowed": True, "name": nome.split("/")[1], "nameWithOwner": nome,
        "owner": {"id": "U_amostra", "login": nome.split("/")[0]}, "parent": None, "rebaseMergeAllowed": True,
        "squashMergeAllowed": True, "url": f"https://github.com/{nome}", "viewerDefaultMergeMethod": "SQUASH",
        "viewerPermission": permissao}


def resumo_checks(total, falhos=0, pendentes=0):
    estado = "Nenhum" if total == 0 else "Pendente" if pendentes else "Falhou" if falhos else "Passou"
    return {"estado": estado, "total": total, "terminados": total - pendentes, "falhos": falhos, "pendentes": pendentes}


def check(repo, nome, balde, workflow="CI", job=None):
    estado = {"pass": "SUCCESS", "fail": "FAILURE", "pending": "IN_PROGRESS", "skipping": "SKIPPED"}[balde]
    c = {"name": nome, "state": estado, "bucket": balde, "workflow": workflow, "description": "", "event": "pull_request",
         "link": f"https://github.com/{repo}/actions/runs/1001/job/{job or abs(hash(nome)) % 100000 + 1000}",
         "startedAt": iso(900), "completedAt": "" if balde == "pending" else iso(600)}
    return c


def comentario(id_, autor, corpo, minutos, assoc="OWNER", tipo="User", **k):
    return {"id": id_, "body": corpo, "created_at": iso(minutos * 60), "html_url": f"https://github.com/{GH}/pull/0#c{id_}",
            "author_association": assoc, "user": {"login": autor, "type": tipo}, **k}


def pr_github(repo, numero, titulo, pr, corpo="", checks=None, gerais=(), em_linha=(), revisoes=(), arquivos=None, issues=()):
    """O que o GitHub diria do PR, coerente com o `pr` do estado (o mesmo head, o mesmo updatedAt,
    os mesmos checks e o último comentário), para a consulta do vigia não mudar nada."""
    estado_gh = {"Aberto": "OPEN", "Rascunho": "OPEN", "Mesclado": "MERGED", "Fechado": "CLOSED"}[pr["estado"]]
    c = pr["checks"]
    contagens = [{"state": "SUCCESS", "count": c["total"] - c["falhos"] - c["pendentes"]}, {"state": "FAILURE", "count": c["falhos"]},
                 {"state": "IN_PROGRESS", "count": c["pendentes"]}]
    no = {
        "number": numero, "state": estado_gh, "isDraft": pr["estado"] == "Rascunho", "url": pr["url"], "headRefName": pr["ramo"],
        "baseRefName": "main", "headRefOid": pr["sha"], "updatedAt": pr["atualizado"], "headRepositoryOwner": {"login": "VictorMenegate"},
        "mergedBy": None, "mergeCommit": None, "mergeable": "CONFLICTING" if pr["mesclavel"] == "DIRTY" else "MERGEABLE",
        "mergeStateStatus": pr["mesclavel"], "reviewDecision": pr["decisao"] or None, "autoMergeRequest": None, "mergeQueueEntry": None,
        "commits": {"nodes": [{"commit": {"oid": pr["sha"], "statusCheckRollup": None if c["total"] == 0 else {
            "state": "FAILURE" if c["falhos"] else "PENDING" if c["pendentes"] else "SUCCESS",
            "contexts": {"totalCount": c["total"], "checkRunCount": c["total"], "checkRunCountsByState": contagens,
                         "statusContextCount": 0, "statusContextCountsByState": []}}}}]},
        "reviewThreads": {"totalCount": pr["conversas_abertas"], "nodes": [{"isResolved": False}] * pr["conversas_abertas"]},
        "comments": {"totalCount": len(gerais), "nodes": [{"databaseId": g["id"]} for g in gerais]},
        "reviews": {"totalCount": len(revisoes), "nodes": [{"databaseId": r["id"], "state": r["state"]} for r in revisoes]},
    }
    view = {
        "number": numero, "title": titulo, "body": corpo, "state": estado_gh, "isDraft": no["isDraft"],
        "author": {"id": "U_amostra", "is_bot": False, "login": "VictorMenegate", "name": "Victor"},
        "createdAt": iso(86400), "updatedAt": pr["atualizado"], "baseRefName": "main", "headRefName": pr["ramo"], "headRefOid": pr["sha"],
        "additions": sum(a["additions"] for a in arquivos or []), "deletions": sum(a["deletions"] for a in arquivos or []),
        "changedFiles": len(arquivos or []), "files": arquivos or [{"path": "extension.js", "additions": 4, "deletions": 1, "changeType": "MODIFIED"}],
        "mergeable": no["mergeable"], "mergeStateStatus": pr["mesclavel"], "reviewDecision": pr["decisao"],
        "closingIssuesReferences": [{"id": f"I_{n}", "number": n, "repository": {"id": "R", "name": repo.split("/")[1], "owner": {"id": "U", "login": repo.split("/")[0]}},
                                     "url": f"https://github.com/{repo}/issues/{n}"} for n in issues],
        "url": pr["url"], "commits": [{"authoredDate": iso(7200), "authors": [{"email": "demo@vitral", "id": "U", "login": "VictorMenegate", "name": "Victor"}],
                                       "committedDate": iso(7200), "messageBody": "", "messageHeadline": titulo, "oid": pr["sha"]}],
        "latestReviews": [], "autoMergeRequest": None, "isCrossRepository": False, "maintainerCanModify": True,
    }
    if gerais:
        pr["ultimo_comentario"] = gerais[-1]["id"]
    if revisoes:
        pr["ultima_revisao"] = revisoes[-1]["id"]
    github["prs"][f"{repo}#{numero}"] = {"no": no, "view": view, "checks": checks or [], "gerais": list(gerais),
                                          "em_linha": list(em_linha), "revisoes": list(revisoes),
                                          "diff": "diff --git a/extension.js b/extension.js\n--- a/extension.js\n+++ b/extension.js\n@@ -9,3 +9,6 @@\n     disable() {\n-        global.display.disconnect(this._sinal);\n+        if (this._sinal) {\n+            global.display.disconnect(this._sinal);\n+        }\n"}


def pr(numero, titulo, repo=GH, ramo=None, checks=(12, 0, 0), **k):
    p = {"repositorio": repo, "numero": numero, "url": f"https://github.com/{repo}/pull/{numero}", "titulo": titulo, "base": "main",
         "ramo": ramo or f"vitral/t{numero}", "cabeca": ramo or f"vitral/t{numero}", "dono_do_head": "VictorMenegate", "estado": "Aberto",
         "sha": sha(numero), "sha_publicado": sha(numero), "commits_de_fora": [], "checks": resumo_checks(*checks),
         "checks_progresso_em": agora - 600, "mesclavel": "CLEAN", "decisao": "", "conversas_abertas": 0, "acompanhando": True,
         "ao_ficar_pronto": None, "empurrado_em": agora - 3600, "conferido_em": agora - 60, "atualizado": iso(3000)}
    p.update(k)
    return p


def tarefa(n, chat_id, titulo, estado="NoPr", **k):
    t = {"id": n, "projeto": PROJ, "coordenador": "", "trabalhador": chat_id, "titulo": titulo, "objetivo": "", "criterios": [],
         "verificacao": None, "estado": estado, "rodada": 1, "worktree": None, "branch": None, "base": None, "copia_limpa": False,
         "progresso": None, "relatorio": None, "ultima_verificacao": None, "criada_em": agora - 86400, "revisao": None,
         "sobreposicao": [], "preparacao": None, "parada": None, "entrega_pendente": None, "preparada": True, "ressalva": None,
         "forma": "Copia", "origem": REPO, "ramo_alvo": "main", "porta": 41000 + n * 10, "atualizada_em": agora, "entrega": None,
         "divisao": None, "lembrete_rodada": 0, "mudancas": None, "juntando_desde": 0, "geracao_preparo": 0, "falha_copia": None,
         "partir_de": "origin/main"}
    t.update(k)
    return t


def chat(chat_id, papel, projeto, n, titulo, cwd, minutos):
    return {"id": chat_id, "projeto": projeto, "titulo": titulo, "papel": papel, "pai": None, "tarefa": n,
            "sessao": f"00000000-0000-4000-8000-{(n or 0):012d}", "sessao_criada": False, "cwd": cwd, "modelo": None,
            "criado_em": agora - 86400, "atualizado_em": agora - minutos * 60, "custo_usd": 0.0, "lido_em": agora}


chats, tarefas, cartoes, itens = [], [], [], {}


def novo(chat_id, n, titulo, minutos, t, conversa, cartao=True):
    chats.append(chat(chat_id, "Projeto", PROJ, n, titulo, os.path.join(COPIAS, f"t{n}"), minutos))
    tarefas.append(t)
    lista = [{"Usuario": {"texto": conversa[0], "anexos": [], "referencias": []}}, {"Assistente": {"texto": conversa[1]}}]
    if cartao:
        cartoes.append({"id": len(cartoes) + 1, "chat": chat_id, "criado_em": agora - 3000, "decidido_em": 0, "estado": "Aberto",
                        "tipo": {"Pr": {"tarefa": n}}})
        lista.append({"Cartao": {"id": len(cartoes)}})
    itens[chat_id] = lista


ENG162 = {"fonte": "Linear", "id": "lin-162", "chave": "ENG-162", "titulo": "Sentry: 39 problemas reais nos 3 primeiros dias",
          "url": "https://linear.app/eita/issue/ENG-162"}
GH12 = {"fonte": "Github", "id": "12", "chave": "#12", "titulo": "A cor da barra não volta ao fechar a última janela",
        "url": f"https://github.com/{GH}/issues/12", "repositorio": GH}

# 1. Checks rodando com 2 falhas, mudanças pedidas e um comentário de quem não tem acesso (Watch).
revisao = {"id": 801, "body": "Funciona, mas o `set_style(null)` no `disable()` apaga o estilo de outras extensões que pintam o painel. Guarda o estilo de antes e devolve ele, por favor.",
           "submitted_at": iso(2400), "html_url": f"https://github.com/{GH}/pull/12#r801", "author_association": "COLLABORATOR",
           "state": "CHANGES_REQUESTED", "commit_id": sha(12), "user": {"login": "colega-do-time", "type": "User"}}
em_linha = comentario(802, "colega-do-time", "Aqui: guarda `Main.panel.get_style()` no `enable()`.", 39, "COLLABORATOR",
                      pull_request_review_id=801, path="extension.js", line=14, original_line=14, commit_id=sha(12),
                      diff_hunk="@@ -9,3 +9,6 @@\n     disable() {\n+        if (this._sinal) {\n             global.display.disconnect(this._sinal);")
terceiro = comentario(803, "mariana-dev", "Isso resolve o meu caso também? Tenho o painel transparente. [log](file:///home/mariana/log.txt)", 20, "NONE")
bot = comentario(804, "cloudflare-workers-and-pages[bot]", "Deploy da prévia pronto.", 50, "NONE", "Bot")
p12 = pr(12, "ENG-162: Corrige o erro do sinal ao desligar a extensão", ramo="vitral/t1-eng-162-sentry", checks=(41, 2, 26),
         decisao="CHANGES_REQUESTED", conversas_abertas=1, mesclavel="BLOCKED",
         esperando=[{"Revisao": {"ids": [801, 802], "autores": ["colega-do-time"]}},
                    {"Terceiro": {"id": 803, "autor": "mariana-dev", "url": f"https://github.com/{GH}/pull/12#c803"}}],
         comentarios_entregues=[801, 802, 803, 804])
checks12 = [check(GH, "build (windows-latest)", "fail", "Unit Tests", 501), check(GH, "lint", "fail", "Lint", 502)]
checks12 += [check(GH, f"e2e shard {i}/26", "pending", "E2E") for i in range(1, 27)]
checks12 += [check(GH, f"unit {i}", "pass", "Unit Tests") for i in range(1, 14)]
pr_github(GH, 12, p12["titulo"], p12, corpo="Corrige o erro que o Sentry acusou 812 vezes.\n\n## O que muda\n- `disable()` só desconecta o sinal quando ele existe;\n- o `enable()` pinta na hora.\n\n## Como foi verificado\n`npm test` passa.\n\nCloses ENG-162. O log bruto está em [file:///var/log/loja.log](file:///var/log/loja.log).",
          checks=checks12, gerais=[bot, terceiro], em_linha=[em_linha], revisoes=[revisao],
          arquivos=[{"path": "extension.js", "additions": 6, "deletions": 2, "changeType": "MODIFIED"},
                    {"path": "tests/disable.test.js", "additions": 41, "deletions": 0, "changeType": "ADDED"}])
github["logs"]["501"] = "\n".join([f"2026-11-01T12:{i:02d}:00Z   build step {i}" for i in range(1, 20)] + [
    "##[error]tests/disable.test.js:12", "  ✕ disable() devolve o estilo anterior (8 ms)", "    Expected: \"background-color: #333;\"",
    "    Received: null", "##[error]Process completed with exit code 1."])
novo("a1000000001", 1, "ENG-162 · Sentry: 39 problemas reais nos 3 primeiros dias", 2,
     tarefa(1, "a1000000001", "ENG-162 · Sentry: 39 problemas reais nos 3 primeiros dias", worktree=t1, branch="vitral/t1-eng-162-sentry",
            base=base, issue=ENG162, pr=p12,
            prs_anteriores=[{"numero": 10, "url": f"https://github.com/{GH}/pull/10", "titulo": "ENG-162: primeira tentativa",
                             "estado": "Fechado", "em": agora - 2 * 86400, "repositorio": GH}]),
     ("corrige o erro do sinal que o Sentry acusa", "Pronto: o `disable()` só desconecta o sinal se ele existe. Abri o PR #12, ligado à ENG-162."))
# 2. Ship ligado, checks rodando.
p13 = pr(13, "Volta a cor da barra quando a última janela fecha", checks=(12, 0, 3), decisao="APPROVED", ao_ficar_pronto="squash")
pr_github(GH, 13, p13["titulo"], p13, corpo="Closes #12.", issues=[12],
          checks=[check(GH, f"unit {i}", "pass") for i in range(1, 10)] + [check(GH, f"e2e {i}", "pending", "E2E") for i in range(1, 4)])
novo("a1000000002", 2, "Cor da barra ao fechar a última janela", 8, tarefa(2, "a1000000002", "Cor da barra ao fechar a última janela", issue=GH12, pr=p13),
     ("a cor da barra não volta ao fechar a última janela (#12)", "Corrigi e abri o PR #13, que fecha a #12; ele mescla quando os checks passarem."))
# 3 a 6. Conflito, atrás de main, rascunho, bloqueado.
for n, numero, titulo, extra, minutos in (
        (3, 14, "Suporte ao GNOME 50", {"mesclavel": "DIRTY"}, 15),
        (4, 15, "Tradução para o espanhol", {"mesclavel": "BEHIND"}, 25),
        (5, 16, "Preferências em GTK4 e Adwaita", {"estado": "Rascunho", "checks": (6, 0, 4)}, 40),
        (6, 17, "Ícone novo da extensão", {"mesclavel": "BLOCKED", "decisao": "REVIEW_REQUIRED"}, 60)):
    p = pr(numero, titulo, **extra)
    lista = [check(GH, f"unit {i}", "pass") for i in range(1, 13)] if p["checks"]["total"] == 12 else \
        [check(GH, "unit 1", "pass"), check(GH, "unit 2", "pass")] + [check(GH, f"e2e {i}", "pending", "E2E") for i in range(1, 5)]
    pr_github(GH, numero, titulo, p, checks=lista)
    novo(f"a100000000{n}", n, titulo, minutos, tarefa(n, f"a100000000{n}", titulo, pr=p), (titulo.lower(), f"Abri o PR #{numero}."))
# 7. Commit de fora e UNSTABLE, com o "Confirmar" de mesclar agora.
p18 = pr(18, "Corrige o vazamento do sinal no disable", mesclavel="UNSTABLE", sha=sha(0x9f3e2a1), sha_publicado=sha(18),
         commits_de_fora=[sha(0x9f3e2a1)[:7]], checks=(8, 0, 0), esperando=[{"CommitDeFora": {"shas": [sha(0x9f3e2a1)[:7]]}}])
# O UNSTABLE vem de um status que não é obrigatório (fora dos checks contados).
pr_github(GH, 18, p18["titulo"], p18, checks=[check(GH, f"unit {i}", "pass") for i in range(1, 9)])
novo("a1000000007", 7, "Corrige o vazamento do sinal", 90, tarefa(7, "a1000000007", "Corrige o vazamento do sinal", pr=p18),
     ("o sinal vaza no disable", "PR #18 aberto; o colega mandou um commit pelo site. Pedi para mesclar."))
cartoes.append({"id": len(cartoes) + 1, "chat": "a1000000007", "criado_em": agora - 600, "decidido_em": 0, "estado": "Aberto",
                "tipo": {"Confirmar": {"acao": {"Mesclar": {"tarefa": 7, "metodo": "squash", "sha": p18["sha"], "quando_pronto": False}},
                                       "pedido_por": "a1000000007", "resumo": "Mesclar agora sempre pede a sua confirmação. O PR tem commits de fora do Vitral (9f3e2a1); confirmar mescla assim mesmo. Checks não obrigatórios falharam; confirmar mescla assim mesmo.",
                                       "resultado": None}}})
itens["a1000000007"].append({"Cartao": {"id": len(cartoes)}})
# 8. Sem permissão para mesclar (o repositório do colega).
p19 = pr(19, "Logs de depuração atrás de uma opção", repo=LEITURA)
pr_github(LEITURA, 19, p19["titulo"], p19, checks=[check(LEITURA, f"unit {i}", "pass") for i in range(1, 13)])
novo("a1000000008", 8, "Logs de depuração", 120, tarefa(8, "a1000000008", "Logs de depuração", pr=p19), ("põe logs de depuração", "PR #19 aberto no repositório do colega."))
# 9. Pronto para mesclar (Watch).
p24 = pr(24, "Atalho para trocar a cor", decisao="APPROVED", esperando=["ProntoParaMesclar"])
pr_github(GH, 24, p24["titulo"], p24, checks=[check(GH, f"unit {i}", "pass") for i in range(1, 13)])
novo("a1000000009", 9, "Atalho para trocar a cor", 150, tarefa(9, "a1000000009", "Atalho para trocar a cor", pr=p24), ("põe um atalho", "PR #24 aberto e aprovado."))
# 10. Mesclado pelo Vitral (nos anteriores) e 11. fechado sem mesclar.
for n, numero, titulo, anterior, minutos in (
        (10, 11, "Login com Google", {"estado": "Mesclado", "mesclado_por": "VictorMenegate", "metodo": "squash", "ramo_apagado": True}, 180),
        (11, 21, "Botão de exportar no Safari", {"estado": "Fechado"}, 240)):
    a = {"numero": numero, "url": f"https://github.com/{GH}/pull/{numero}", "titulo": titulo, "em": agora - 3600, "repositorio": GH, "sha": sha(numero)}
    a.update(anterior)
    p = pr(numero, titulo, estado=anterior["estado"])
    pr_github(GH, numero, titulo, p, checks=[check(GH, f"unit {i}", "pass") for i in range(1, 13)])
    novo(f"a10000000{n:02d}", n, titulo, minutos, tarefa(n, f"a10000000{n:02d}", titulo, estado="Conversa", rodada=2, prs_anteriores=[a]),
         (titulo.lower(), f"Abri o PR #{numero}."))
# 12. A cópia com mudanças, antes do PR ("Revisar e abrir PR").
novo("a1000000012", 12, "Cor da barra sem janela focada", 1,
     tarefa(12, "a1000000012", "Cor da barra sem janela focada", estado="Conversa", worktree=t12, branch="vitral/t12-cor-da-barra-sem-janela",
            base=base, mudancas={"arquivos": 2, "mais": 9, "menos": 2}),
     ("sem janela focada, a barra fica com a cor do último app", "Pronto: sem janela focada, o painel volta à cor do tema. Mudei `extension.js` e criei `novo-cores.js`."),
     cartao=False)
# 13. CI falhou e a rodada que corrige espera o Atualizar PR.
p20 = pr(20, "Dark mode no painel", checks=(12, 1, 0), esperando=[{"CiFalhou": {"chave": "x", "falhos": ["test (ubuntu)"]}}])
pr_github(GH, 20, p20["titulo"], p20, checks=[check(GH, "test (ubuntu)", "fail", "Unit Tests", 701)] + [check(GH, f"unit {i}", "pass") for i in range(1, 12)])
novo("a1000000013", 13, "Dark mode no painel", 300, tarefa(13, "a1000000013", "Dark mode no painel", pronto_para_atualizar=True, rodada=2, pr=p20),
     ("o CI falhou, corrige", "Corrigi o teste que falhava; a verificação passou e a rodada espera o Atualizar PR."))
for t in tarefas:
    # A falha do CI já avisada neste head (o vigia não avisa de novo) e desde quando.
    p = t.get("pr")
    if p and p["checks"]["falhos"]:
        p["falha_avisada"] = p["sha"] + " amostra"
        p["primeira_falha_em"] = agora - 3600

# ---------- as issues do GitHub ----------
def issue(n, titulo, estado="OPEN", autor="colega-do-time", rotulos=()):
    return {"assignees": [], "author": {"id": "U", "is_bot": False, "login": autor, "name": autor}, "createdAt": iso(5 * 86400),
            "labels": [{"id": f"L{r}", "name": r, "description": "", "color": "D6393F"} for r in rotulos], "number": n, "state": estado,
            "stateReason": "", "title": titulo, "updatedAt": iso(86400), "url": f"https://github.com/{GH}/issues/{n}"}
github["issues"][GH] = [issue(12, GH12["titulo"], rotulos=("bug",)), issue(30, "Suporte a temas claros", rotulos=("ideia",)),
                        issue(31, "A cor pisca ao trocar de espaço de trabalho"), issue(9, "Cor errada no Firefox", "CLOSED")]
for i in github["issues"][GH]:
    github["issue"][f"{GH}#{i['number']}"] = dict(i, body="Ao fechar a última janela, a barra continua com a cor do app.\n\n- [ ] volta à cor do tema",
                                                   comments=[], milestone=None, closedByPullRequestsReferences=[])

# ---------- grava ----------
mestre = chat("mestre", "Mestre", "", None, "Mestre", os.path.join(D, "mestre"), 24 * 60)
os.makedirs(os.path.join(D, "mestre"), exist_ok=True)
os.makedirs(os.path.join(D, "chats"), exist_ok=True)
itens["mestre"] = [{"Aviso": {"texto": "Sou o Mestre: me diga o que quer fazer.", "erro": False}}]
projeto = {"id": PROJ, "nome": "top-bar-app-color", "cor": "#c8b98f",
           "pastas": [{"caminho": REPO, "github": {"entrega": "Pr", "repositorio": GH}}], "criado_em": agora - 10 * 86400}
estado = {"versao": 2, "projetos": [projeto], "chats": [mestre] + chats, "tarefas": tarefas, "proxima_tarefa": 14, "filas": {},
          "rotinas": [], "cartoes": cartoes, "proximo_cartao": len(cartoes) + 1, "ocorrencias": [], "visto_em": agora,
          "claude_code": {"importadas": [], "pendente": False, "ultima_vez": agora}}
for chat_id, lista in itens.items():
    json.dump(lista, open(os.path.join(D, "chats", f"{chat_id}.json"), "w"), indent=1, ensure_ascii=False)
json.dump(estado, open(os.path.join(D, "estado.json"), "w"), indent=1, ensure_ascii=False)
open(os.path.join(D, "formato"), "w").write("2\n")
json.dump(github, open(os.path.join(A, "github.json"), "w"), indent=1, ensure_ascii=False)
os.makedirs(os.path.join(B, "config", "vitral"), exist_ok=True)
json.dump({"conexoes": {"linear": None}, "permissao": "bypassPermissions"}, open(os.path.join(B, "config", "vitral", "config.json"), "w"), indent=2)
# O zoom que o ZOOM= da bancada gravou fica; o chat que abre é o do AMOSTRA_CHAT.
caminho = os.path.join(B, "config", "vitral", "interface.json")
interface = json.load(open(caminho)) if os.path.exists(caminho) else {}
interface["ultimo_chat"] = os.environ.get("AMOSTRA_CHAT", "a1000000001")
json.dump(interface, open(caminho, "w"))
print(f"amostra de PRs em {B}: {len(tarefas)} chats, {len(github['prs'])} PRs, {len(github['issues'][GH])} issues")
