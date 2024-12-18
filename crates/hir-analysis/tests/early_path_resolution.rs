mod test_db;
use std::path::Path;

use dir_test::{dir_test, Fixture};
use fe_compiler_test_utils::snap_test;
use fe_hir_analysis::{
    name_resolution::{resolve_path_early, EarlyResolvedPath, NameDomain},
    HirAnalysisDb,
};
use hir::{
    hir_def::{Expr, ExprId, ItemKind, Pat, PatId, PathId, TopLevelMod, TypeId},
    visitor::prelude::*,
    HirDb, SpannedHirDb,
};
use test_db::{HirAnalysisTestDb, HirPropertyFormatter};

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/test_files/early_path_resolution",
    glob: "*.fe"
)]
fn early_path_resolution_standalone(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let path = Path::new(fixture.path());
    let file_name = path.file_name().and_then(|file| file.to_str()).unwrap();
    let input = db.new_stand_alone(file_name, fixture.content());
    let (top_mod, mut prop_formatter) = db.top_mod(input);
    db.assert_no_diags(top_mod);

    let mut ctxt = VisitorCtxt::with_top_mod(db.as_hir_db(), top_mod);
    PathVisitor {
        db: &db,
        top_mod,
        domain_stack: Vec::new(),
        prop_formatter: &mut prop_formatter,
    }
    .visit_top_mod(&mut ctxt, top_mod);

    let res = prop_formatter.finish(db.as_spanned_hir_db());
    snap_test!(res, fixture.path());
}

struct PathVisitor<'db, 'a> {
    db: &'db HirAnalysisTestDb,
    top_mod: TopLevelMod<'db>,
    domain_stack: Vec<NameDomain>,
    prop_formatter: &'a mut HirPropertyFormatter<'db>,
}

impl<'db> Visitor<'db> for PathVisitor<'db, '_> {
    fn visit_item(&mut self, ctxt: &mut VisitorCtxt<'db, LazyItemSpan<'db>>, item: ItemKind<'db>) {
        if matches!(item, ItemKind::Use(_)) {
            return;
        }

        self.domain_stack.push(NameDomain::TYPE);
        walk_item(self, ctxt, item);
        self.domain_stack.pop();
    }

    fn visit_ty(&mut self, ctxt: &mut VisitorCtxt<'db, LazyTySpan<'db>>, ty: TypeId<'db>) {
        self.domain_stack.push(NameDomain::TYPE);
        walk_ty(self, ctxt, ty);
        self.domain_stack.pop();
    }

    fn visit_pat(&mut self, _: &mut VisitorCtxt<'db, LazyPatSpan<'db>>, _: PatId, _: &Pat<'db>) {}

    fn visit_expr(
        &mut self,
        ctxt: &mut VisitorCtxt<'db, LazyExprSpan<'db>>,
        expr: ExprId,
        expr_data: &Expr<'db>,
    ) {
        if matches!(expr_data, Expr::Block { .. }) {
            walk_expr(self, ctxt, expr);
        }
    }

    fn visit_path(&mut self, ctxt: &mut VisitorCtxt<'db, LazyPathSpan<'db>>, path: PathId<'db>) {
        let scope = ctxt.scope();
        match resolve_path_early(self.db, path, scope).unwrap() {
            EarlyResolvedPath::Full(bucket) => {
                let domain = self.domain_stack.last().copied().unwrap();
                let res = bucket.pick(domain).as_ref().unwrap();
                let prop = res.pretty_path(self.db.as_hir_analysis_db()).unwrap();
                let span = ctxt
                    .span()
                    .unwrap()
                    .segment(path.segment_index(self.db.as_hir_db()))
                    .ident()
                    .into();
                self.prop_formatter.push_prop(self.top_mod, span, prop);
            }

            EarlyResolvedPath::Partial { path, res } => {
                let prop = res.pretty_path(self.db.as_hir_analysis_db()).unwrap();
                let span = ctxt
                    .span()
                    .unwrap()
                    .segment(path.segment_index(self.db))
                    .ident()
                    .into();
                self.prop_formatter.push_prop(self.top_mod, span, prop);
            }
        }
        walk_path(self, ctxt, path);
    }
}
